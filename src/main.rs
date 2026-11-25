#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::{Local, NaiveDate};
use diario::armazenamento::{self, Diario, Entrada};
use diario::audio::{self, Gravacao, Microfone};
use diario::modelo;
use diario::transcricao::Transcritor;
use eframe::egui::{self, Align, Color32, Key, Layout, Modifiers, RichText};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

const VERMELHO: Color32 = Color32::from_rgb(200, 45, 45);
const AZUL: Color32 = Color32::from_rgb(40, 110, 200);

fn main() -> eframe::Result {
    let opcoes = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Diário")
            .with_inner_size([820.0, 680.0])
            .with_min_inner_size([560.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Diário",
        opcoes,
        Box::new(|cc| Ok(Box::new(App::new(cc.egui_ctx.clone())))),
    )
}

enum EstadoModelo {
    Baixando { baixado: u64, total: Option<u64> },
    Carregando,
    Pronto(Arc<Transcritor>),
    Erro(String),
}

enum Evento {
    Progresso { baixado: u64, total: Option<u64> },
    Carregando,
    ModeloPronto(Transcritor),
    ModeloErro(String),
    Transcrito(Result<String, String>),
}

enum Aviso {
    Info(String),
    Erro(String),
}

enum Acao {
    Editar(usize),
    PedirApagar(usize),
    CancelarApagar,
    Apagar(usize),
}

struct App {
    ctx: egui::Context,
    pasta: PathBuf,
    diario: Diario,
    dia: NaiveDate,
    entradas: Vec<Entrada>,
    dias: Vec<NaiveDate>,

    rascunho: String,
    rascunho_gravado: String,
    editando: Option<usize>,
    confirmar_apagar: Option<usize>,

    microfones: Vec<Microfone>,
    microfone: Option<String>,
    gravacao: Option<Gravacao>,
    /// Áudio gravado antes de o modelo ficar pronto; transcreve assim que der.
    pendente: Option<Vec<f32>>,
    transcrevendo: bool,
    modelo: EstadoModelo,

    aviso: Option<Aviso>,
    tx: Sender<Evento>,
    rx: Receiver<Evento>,
}

impl App {
    fn new(ctx: egui::Context) -> Self {
        let pasta = diario::pasta_base();
        let diario = Diario::new(pasta.join("entradas"));
        let dia = hoje();
        let (tx, rx) = mpsc::channel();
        let rascunho = std::fs::read_to_string(pasta.join("rascunho.txt")).unwrap_or_default();
        let microfone = std::fs::read_to_string(pasta.join("microfone.txt"))
            .ok()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty());

        let mut app = Self {
            ctx,
            diario,
            dia,
            entradas: Vec::new(),
            dias: Vec::new(),
            rascunho_gravado: rascunho.clone(),
            rascunho,
            editando: None,
            confirmar_apagar: None,
            microfones: audio::listar_microfones(),
            microfone,
            gravacao: None,
            pendente: None,
            transcrevendo: false,
            modelo: EstadoModelo::Carregando,
            aviso: None,
            tx,
            rx,
            pasta,
        };
        app.abrir_dia(dia);
        app.preparar_modelo();
        app
    }

    // ----- modelo e transcrição -------------------------------------------------

    fn preparar_modelo(&mut self) {
        let caminho = modelo::caminho(&self.pasta);
        let nome = modelo::nome();
        self.modelo = if caminho.exists() {
            EstadoModelo::Carregando
        } else {
            EstadoModelo::Baixando { baixado: 0, total: None }
        };
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let enviar = |evento| {
                let _ = tx.send(evento);
                ctx.request_repaint();
            };
            if !caminho.exists() {
                let mut ultimo_mb = u64::MAX;
                let resultado = modelo::baixar(&modelo::url(&nome), &caminho, |baixado, total| {
                    let mb = baixado / (1024 * 1024);
                    if mb != ultimo_mb {
                        ultimo_mb = mb;
                        enviar(Evento::Progresso { baixado, total });
                    }
                });
                if let Err(erro) = resultado {
                    enviar(Evento::ModeloErro(erro));
                    return;
                }
            }
            enviar(Evento::Carregando);
            match Transcritor::carregar(&caminho) {
                Ok(transcritor) => enviar(Evento::ModeloPronto(transcritor)),
                Err(erro) => enviar(Evento::ModeloErro(erro)),
            }
        });
    }

    fn transcrever(&mut self, audio: Vec<f32>) {
        let EstadoModelo::Pronto(transcritor) = &self.modelo else {
            self.pendente = Some(audio);
            return;
        };
        let transcritor = transcritor.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        self.transcrevendo = true;
        std::thread::spawn(move || {
            let _ = tx.send(Evento::Transcrito(transcritor.transcrever(&audio)));
            ctx.request_repaint();
        });
    }

    fn processar_eventos(&mut self) {
        while let Ok(evento) = self.rx.try_recv() {
            match evento {
                Evento::Progresso { baixado, total } => {
                    self.modelo = EstadoModelo::Baixando { baixado, total };
                }
                Evento::Carregando => self.modelo = EstadoModelo::Carregando,
                Evento::ModeloPronto(transcritor) => {
                    self.modelo = EstadoModelo::Pronto(Arc::new(transcritor));
                    if let Some(audio) = self.pendente.take() {
                        self.transcrever(audio);
                    }
                }
                Evento::ModeloErro(erro) => self.modelo = EstadoModelo::Erro(erro),
                Evento::Transcrito(resultado) => {
                    self.transcrevendo = false;
                    match resultado {
                        Ok(texto) if texto.trim().is_empty() => {
                            self.aviso = Some(Aviso::Info(
                                "Não entendi nenhuma fala nessa gravação. Tente de novo, mais perto do microfone."
                                    .into(),
                            ));
                        }
                        Ok(texto) => {
                            let rascunho = self.rascunho.trim_end().to_string();
                            self.rascunho = if rascunho.is_empty() {
                                texto.trim().to_string()
                            } else {
                                format!("{rascunho} {}", texto.trim())
                            };
                        }
                        Err(erro) => self.aviso = Some(Aviso::Erro(erro)),
                    }
                }
            }
        }
    }

    // ----- gravação ---------------------------------------------------------------

    fn alternar_gravacao(&mut self) {
        match self.gravacao.take() {
            None => match Gravacao::iniciar(self.microfone.as_deref()) {
                Ok(gravacao) => {
                    self.aviso = None;
                    self.gravacao = Some(gravacao);
                }
                Err(erro) => self.aviso = Some(Aviso::Erro(erro)),
            },
            Some(gravacao) => match gravacao.parar() {
                Ok(audio) => {
                    let segundos = audio.len() as f32 / audio::TAXA_WHISPER as f32;
                    let pico = audio.iter().fold(0f32, |m, s| m.max(s.abs()));
                    if segundos < 0.5 {
                        self.aviso = Some(Aviso::Info("A gravação ficou curta demais.".into()));
                    } else if pico < 0.003 {
                        self.aviso = Some(Aviso::Erro(
                            "Não captei som nenhum. Confira o microfone escolhido no canto de cima."
                                .into(),
                        ));
                    } else {
                        self.transcrever(audio);
                    }
                }
                Err(erro) => self.aviso = Some(Aviso::Erro(erro)),
            },
        }
    }

    fn escolher_microfone(&mut self, id: Option<String>) {
        if self.microfone == id {
            return;
        }
        self.microfone = id;
        let arquivo = self.pasta.join("microfone.txt");
        let resultado = std::fs::create_dir_all(&self.pasta)
            .and_then(|_| std::fs::write(&arquivo, self.microfone.clone().unwrap_or_default()));
        if let Err(erro) = resultado {
            self.aviso = Some(Aviso::Erro(format!("Não consegui lembrar o microfone escolhido: {erro}")));
        }
    }

    // ----- anotações ----------------------------------------------------------------

    fn abrir_dia(&mut self, dia: NaiveDate) {
        self.dia = dia;
        self.editando = None;
        self.confirmar_apagar = None;
        match self.diario.carregar(dia) {
            Ok(entradas) => self.entradas = entradas,
            Err(erro) => {
                self.entradas = Vec::new();
                self.aviso = Some(Aviso::Erro(format!(
                    "Não consegui ler {}: {erro}",
                    self.diario.arquivo(dia).display()
                )));
            }
        }
        self.dias = self.diario.dias_com_anotacoes();
    }

    fn salvar_rascunho(&mut self) {
        let texto = self.rascunho.trim().to_string();
        if texto.is_empty() {
            return;
        }
        let anteriores = self.entradas.clone();
        match self.editando {
            Some(i) if i < self.entradas.len() => self.entradas[i].texto = texto,
            _ => {
                self.entradas.push(Entrada {
                    hora: Local::now().format("%H:%M").to_string(),
                    texto,
                });
                self.entradas.sort_by(|a, b| a.hora.cmp(&b.hora));
            }
        }
        match self.diario.salvar(self.dia, &self.entradas) {
            Ok(()) => {
                self.rascunho.clear();
                self.editando = None;
                self.dias = self.diario.dias_com_anotacoes();
            }
            Err(erro) => {
                self.entradas = anteriores;
                self.aviso = Some(Aviso::Erro(format!("Não consegui salvar a anotação: {erro}")));
            }
        }
    }

    fn aplicar(&mut self, acao: Acao) {
        match acao {
            Acao::Editar(i) => {
                if self.editando.is_none() && !self.rascunho.trim().is_empty() {
                    self.aviso = Some(Aviso::Info(
                        "Salve ou limpe o texto que está na caixa antes de editar outra anotação.".into(),
                    ));
                    return;
                }
                if let Some(entrada) = self.entradas.get(i) {
                    self.rascunho = entrada.texto.clone();
                    self.editando = Some(i);
                }
            }
            Acao::PedirApagar(i) => self.confirmar_apagar = Some(i),
            Acao::CancelarApagar => self.confirmar_apagar = None,
            Acao::Apagar(i) => {
                self.confirmar_apagar = None;
                if i >= self.entradas.len() {
                    return;
                }
                let anteriores = self.entradas.clone();
                self.entradas.remove(i);
                match self.diario.salvar(self.dia, &self.entradas) {
                    Ok(()) => {
                        match self.editando {
                            Some(e) if e == i => {
                                self.editando = None;
                                self.rascunho.clear();
                            }
                            Some(e) if e > i => self.editando = Some(e - 1),
                            _ => {}
                        }
                        self.dias = self.diario.dias_com_anotacoes();
                    }
                    Err(erro) => {
                        self.entradas = anteriores;
                        self.aviso = Some(Aviso::Erro(format!("Não consegui apagar: {erro}")));
                    }
                }
            }
        }
    }

    /// Guarda o texto da caixa em disco, para não perder uma transcrição se o app fechar.
    fn guardar_rascunho_em_disco(&mut self) {
        if self.rascunho == self.rascunho_gravado {
            return;
        }
        let arquivo = self.pasta.join("rascunho.txt");
        let resultado = std::fs::create_dir_all(&self.pasta).and_then(|_| {
            if self.rascunho.is_empty() {
                std::fs::remove_file(&arquivo).or_else(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) }
                })
            } else {
                std::fs::write(&arquivo, &self.rascunho)
            }
        });
        if resultado.is_ok() {
            self.rascunho_gravado = self.rascunho.clone();
        }
    }

    // ----- telas ------------------------------------------------------------------

    fn barra_topo(&mut self, ui: &mut egui::Ui) {
        let hoje = hoje();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading("📔 Diário");
            ui.separator();
            if ui.button("◀").on_hover_text("Dia anterior").clicked() {
                if let Some(anterior) = self.dia.pred_opt() {
                    self.abrir_dia(anterior);
                }
            }
            let rotulo = format!(
                "{} · {}",
                armazenamento::nome_relativo(self.dia, hoje),
                self.dia.format("%d/%m/%Y")
            );
            ui.label(RichText::new(rotulo).strong().size(16.0));
            if ui
                .add_enabled(self.dia < hoje, egui::Button::new("▶"))
                .on_hover_text("Próximo dia")
                .clicked()
            {
                if let Some(proximo) = self.dia.succ_opt() {
                    self.abrir_dia(proximo);
                }
            }
            if ui.add_enabled(self.dia != hoje, egui::Button::new("Hoje")).clicked() {
                self.abrir_dia(hoje);
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("🔄").on_hover_text("Procurar microfones de novo").clicked() {
                    self.microfones = audio::listar_microfones();
                }
                let selecionado = self
                    .microfone
                    .as_ref()
                    .and_then(|id| self.microfones.iter().find(|m| &m.id == id))
                    .map_or("Microfone padrão do Windows".to_string(), |m| m.nome.clone());
                let mut escolha = self.microfone.clone();
                ui.add_enabled_ui(self.gravacao.is_none(), |ui| {
                    egui::ComboBox::from_id_salt("microfone")
                        .width(240.0)
                        .selected_text(selecionado)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut escolha, None, "Microfone padrão do Windows");
                            for microfone in &self.microfones {
                                ui.selectable_value(&mut escolha, Some(microfone.id.clone()), &microfone.nome);
                            }
                        });
                });
                self.escolher_microfone(escolha);
                ui.label("🎙");
            });
        });
        ui.add_space(4.0);
    }

    fn lista_dias(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.label(RichText::new("Dias anotados").strong());
        ui.add_space(4.0);
        let mut ir_para = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if self.dias.is_empty() {
                ui.weak("Nenhum ainda.");
            }
            for &dia in &self.dias {
                let texto = format!(
                    "{}  {}",
                    dia.format("%d/%m/%y"),
                    armazenamento::dia_semana_curto(dia)
                );
                if ui.selectable_label(dia == self.dia, texto).clicked() {
                    ir_para = Some(dia);
                }
            }
        });
        if let Some(dia) = ir_para {
            self.abrir_dia(dia);
        }
    }

    fn painel_modelo(&mut self, ui: &mut egui::Ui) {
        match &self.modelo {
            EstadoModelo::Pronto(_) => {}
            EstadoModelo::Carregando => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Carregando o reconhecimento de voz…");
                });
            }
            EstadoModelo::Baixando { baixado, total } => {
                let mb = |b: u64| b as f32 / (1024.0 * 1024.0);
                let (fracao, texto) = match total {
                    Some(total) if *total > 0 => (
                        *baixado as f32 / *total as f32,
                        format!("{:.0} de {:.0} MB", mb(*baixado), mb(*total)),
                    ),
                    _ => (0.0, format!("{:.0} MB", mb(*baixado))),
                };
                ui.label(format!(
                    "Primeira vez: baixando o modelo de voz ({}). Isso acontece uma vez só. \
                     Você já pode gravar; o texto aparece quando o download terminar.",
                    modelo::nome()
                ));
                ui.add(egui::ProgressBar::new(fracao).text(texto).animate(true));
            }
            EstadoModelo::Erro(erro) => {
                let erro = erro.clone();
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(VERMELHO, format!("⚠ {erro}"));
                    if ui.button("Tentar de novo").clicked() {
                        self.preparar_modelo();
                    }
                });
            }
        }
    }

    fn centro(&mut self, ui: &mut egui::Ui) {
        let hoje = hoje();
        self.painel_modelo(ui);

        // Botão do microfone
        ui.add_space(8.0);
        let gravando = self.gravacao.is_some();
        let aguardando_modelo = self.pendente.is_some();
        ui.vertical_centered(|ui| {
            let (texto, cor) = if gravando {
                ("⏹  Parar e transcrever", VERMELHO)
            } else if self.transcrevendo {
                ("⏳  Transcrevendo…", Color32::GRAY)
            } else if aguardando_modelo {
                ("⏳  Esperando o modelo…", Color32::GRAY)
            } else {
                ("🎤  Falar", AZUL)
            };
            let botao = egui::Button::new(RichText::new(texto).size(22.0).color(Color32::WHITE))
                .fill(cor)
                .min_size(egui::vec2(300.0, 58.0))
                .corner_radius(29.0);
            let livre = !self.transcrevendo && !aguardando_modelo;
            if ui
                .add_enabled(livre || gravando, botao)
                .on_hover_text("Clique, fale o que fez e clique de novo para virar texto.")
                .clicked()
            {
                self.alternar_gravacao();
            }

            if let Some(gravacao) = &self.gravacao {
                let segundos = gravacao.duracao().as_secs();
                let nivel = gravacao.nivel().sqrt().clamp(0.0, 1.0);
                ui.add_space(4.0);
                ui.add(
                    egui::ProgressBar::new(nivel)
                        .desired_width(300.0)
                        .fill(VERMELHO)
                        .text(format!("● gravando  {:02}:{:02}", segundos / 60, segundos % 60)),
                );
            } else if self.transcrevendo {
                ui.add_space(4.0);
                ui.spinner();
            }
        });

        // Avisos
        if let Some(aviso) = &self.aviso {
            ui.add_space(6.0);
            let mut fechar = false;
            ui.horizontal_wrapped(|ui| {
                match aviso {
                    Aviso::Info(texto) => ui.label(format!("ℹ {texto}")),
                    Aviso::Erro(texto) => ui.colored_label(VERMELHO, format!("⚠ {texto}")),
                };
                fechar = ui.small_button("✖").clicked();
            });
            if fechar {
                self.aviso = None;
            }
        }

        // Caixa de texto da anotação
        ui.add_space(10.0);
        let rotulo = match self.editando.and_then(|i| self.entradas.get(i)) {
            Some(entrada) => format!("Editando a anotação das {}", entrada.hora),
            None if self.dia == hoje => "Nova anotação".to_string(),
            None => format!(
                "Nova anotação em {} ({})",
                self.dia.format("%d/%m"),
                armazenamento::nome_dia_semana(self.dia)
            ),
        };
        ui.label(RichText::new(rotulo).strong());
        // Lê o atalho antes da caixa de texto, para o Ctrl+Enter não virar quebra de linha.
        let atalho_salvar = ui.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Enter));
        ui.add(
            egui::TextEdit::multiline(&mut self.rascunho)
                .hint_text("Clique em Falar e conte o que você fez… ou digite aqui.")
                .desired_rows(4)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            let tem_texto = !self.rascunho.trim().is_empty();
            let texto_salvar = if self.editando.is_some() {
                "💾 Salvar alteração"
            } else {
                "💾 Salvar no diário"
            };
            let salvar = ui
                .add_enabled(tem_texto, egui::Button::new(texto_salvar))
                .on_hover_text("Ctrl+Enter");
            if salvar.clicked() || (atalho_salvar && tem_texto) {
                self.salvar_rascunho();
            }
            let texto_limpar = if self.editando.is_some() { "Cancelar edição" } else { "Limpar" };
            if ui
                .add_enabled(tem_texto || self.editando.is_some(), egui::Button::new(texto_limpar))
                .clicked()
            {
                self.rascunho.clear();
                self.editando = None;
            }
        });

        // Anotações do dia
        ui.add_space(10.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.heading(format!(
                "Anotações de {}",
                armazenamento::nome_relativo(self.dia, hoje).to_lowercase()
            ));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(!self.entradas.is_empty(), egui::Button::new("📋 Copiar para a daily"))
                    .on_hover_text("Copia as anotações deste dia como lista, para colar no chat")
                    .clicked()
                {
                    ui.ctx()
                        .copy_text(armazenamento::texto_para_daily(self.dia, hoje, &self.entradas));
                    self.aviso = Some(Aviso::Info("Copiado. É só colar no chat da daily.".into()));
                }
            });
        });
        ui.add_space(4.0);

        let mut acao = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if self.entradas.is_empty() {
                ui.weak("Nenhuma anotação neste dia ainda.");
            }
            for (i, entrada) in self.entradas.iter().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&entrada.hora).strong().monospace());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if self.confirmar_apagar == Some(i) {
                                if ui.small_button("Cancelar").clicked() {
                                    acao = Some(Acao::CancelarApagar);
                                }
                                if ui
                                    .small_button(RichText::new("Apagar mesmo").color(VERMELHO))
                                    .clicked()
                                {
                                    acao = Some(Acao::Apagar(i));
                                }
                            } else {
                                if ui.small_button("🗑").on_hover_text("Apagar").clicked() {
                                    acao = Some(Acao::PedirApagar(i));
                                }
                                if ui.small_button("✏").on_hover_text("Editar").clicked() {
                                    acao = Some(Acao::Editar(i));
                                }
                            }
                        });
                    });
                    ui.label(&entrada.texto);
                });
                ui.add_space(2.0);
            }
            ui.add_space(6.0);
            ui.weak(format!("Arquivo: {}", self.diario.arquivo(self.dia).display()));
        });
        if let Some(acao) = acao {
            self.aplicar(acao);
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.processar_eventos();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("topo").show(ui, |ui| self.barra_topo(ui));
        egui::Panel::left("dias")
            .resizable(false)
            .exact_size(140.0)
            .show(ui, |ui| self.lista_dias(ui));
        egui::CentralPanel::default().show(ui, |ui| self.centro(ui));

        self.guardar_rascunho_em_disco();

        let ocupado = self.gravacao.is_some()
            || self.transcrevendo
            || matches!(self.modelo, EstadoModelo::Baixando { .. } | EstadoModelo::Carregando);
        if ocupado {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
    }
}

fn hoje() -> NaiveDate {
    Local::now().date_naive()
}
