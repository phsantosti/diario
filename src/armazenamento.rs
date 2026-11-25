//! Anotações em Markdown, um arquivo por dia: `entradas/AAAA-MM-DD.md`.
//!
//! Formato:
//!
//! ```text
//! # Diário de 20/11/2026 (sexta-feira)
//!
//! ## 09:15
//! Revisei o PR da tela de pedidos.
//!
//! ## 14:30
//! Reunião com o suporte sobre o chamado do relatório.
//! ```
//!
//! O arquivo pode ser aberto e editado à mão; o app relê ao trocar de dia.

use chrono::{Datelike, NaiveDate, Weekday};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Cabeçalho usado para texto que estava no arquivo sem um `## HH:MM` antes.
const SEM_HORA: &str = "(sem hora)";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entrada {
    /// "HH:MM" de quando a anotação foi salva.
    pub hora: String,
    pub texto: String,
}

pub struct Diario {
    pasta: PathBuf,
}

impl Diario {
    pub fn new(pasta: PathBuf) -> Self {
        Self { pasta }
    }

    pub fn pasta(&self) -> &Path {
        &self.pasta
    }

    pub fn arquivo(&self, dia: NaiveDate) -> PathBuf {
        self.pasta.join(format!("{}.md", dia.format("%Y-%m-%d")))
    }

    /// Lê as anotações de um dia. Dia sem arquivo é dia sem anotações.
    pub fn carregar(&self, dia: NaiveDate) -> io::Result<Vec<Entrada>> {
        match fs::read_to_string(self.arquivo(dia)) {
            Ok(conteudo) => Ok(ler(&conteudo)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// Grava as anotações do dia. Escreve num arquivo temporário e troca no fim,
    /// para nunca deixar o arquivo do dia pela metade.
    pub fn salvar(&self, dia: NaiveDate, entradas: &[Entrada]) -> io::Result<()> {
        let arquivo = self.arquivo(dia);
        if entradas.is_empty() {
            return match fs::remove_file(&arquivo) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        fs::create_dir_all(&self.pasta)?;
        let temporario = arquivo.with_extension("md.tmp");
        fs::write(&temporario, escrever(dia, entradas))?;
        fs::rename(&temporario, &arquivo)
    }

    /// Dias que têm arquivo de anotações, do mais recente para o mais antigo.
    pub fn dias_com_anotacoes(&self) -> Vec<NaiveDate> {
        let Ok(itens) = fs::read_dir(&self.pasta) else {
            return Vec::new();
        };
        let mut dias: Vec<NaiveDate> = itens
            .filter_map(|item| {
                let caminho = item.ok()?.path();
                if caminho.extension()? != "md" {
                    return None;
                }
                NaiveDate::parse_from_str(caminho.file_stem()?.to_str()?, "%Y-%m-%d").ok()
            })
            .collect();
        dias.sort_unstable_by(|a, b| b.cmp(a));
        dias
    }
}

/// Converte o Markdown do dia em anotações.
pub fn ler(conteudo: &str) -> Vec<Entrada> {
    let mut entradas: Vec<Entrada> = Vec::new();
    let mut atual: Option<Entrada> = None;
    let mut antes_da_primeira = String::new();
    let mut viu_titulo = false;

    for linha in conteudo.lines() {
        if let Some(hora) = linha.strip_prefix("## ") {
            if let Some(entrada) = atual.take() {
                entradas.push(entrada);
            }
            atual = Some(Entrada {
                hora: hora.trim().to_string(),
                texto: String::new(),
            });
            continue;
        }
        match atual.as_mut() {
            Some(entrada) => {
                entrada.texto.push_str(linha);
                entrada.texto.push('\n');
            }
            None => {
                // O título "# ..." é gerado pelo app; o resto não pode se perder.
                if !viu_titulo && linha.starts_with("# ") && antes_da_primeira.trim().is_empty() {
                    viu_titulo = true;
                } else {
                    antes_da_primeira.push_str(linha);
                    antes_da_primeira.push('\n');
                }
            }
        }
    }
    if let Some(entrada) = atual {
        entradas.push(entrada);
    }

    if !antes_da_primeira.trim().is_empty() {
        entradas.insert(
            0,
            Entrada {
                hora: SEM_HORA.to_string(),
                texto: antes_da_primeira,
            },
        );
    }
    for entrada in &mut entradas {
        entrada.texto = entrada.texto.trim().to_string();
    }
    entradas
}

/// Converte as anotações do dia em Markdown.
pub fn escrever(dia: NaiveDate, entradas: &[Entrada]) -> String {
    let mut saida = format!(
        "# Diário de {} ({})\n",
        dia.format("%d/%m/%Y"),
        nome_dia_semana(dia)
    );
    for entrada in entradas {
        let hora = if entrada.hora.trim().is_empty() {
            SEM_HORA
        } else {
            entrada.hora.trim()
        };
        saida.push_str(&format!("\n## {hora}\n{}\n", entrada.texto.trim()));
    }
    saida
}

/// Texto pronto para colar no chat da daily.
pub fn texto_para_daily(dia: NaiveDate, hoje: NaiveDate, entradas: &[Entrada]) -> String {
    let mut saida = format!("{} ({}):\n", nome_relativo(dia, hoje), dia.format("%d/%m"));
    for entrada in entradas {
        let linha = entrada.texto.split_whitespace().collect::<Vec<_>>().join(" ");
        saida.push_str(&format!("- {linha}\n"));
    }
    saida
}

/// "Hoje", "Ontem" ou o dia da semana com inicial maiúscula.
pub fn nome_relativo(dia: NaiveDate, hoje: NaiveDate) -> String {
    if dia == hoje {
        return "Hoje".to_string();
    }
    if Some(dia) == hoje.pred_opt() {
        return "Ontem".to_string();
    }
    let nome = nome_dia_semana(dia);
    let mut letras = nome.chars();
    match letras.next() {
        Some(primeira) => primeira.to_uppercase().chain(letras).collect(),
        None => String::new(),
    }
}

pub fn nome_dia_semana(dia: NaiveDate) -> &'static str {
    match dia.weekday() {
        Weekday::Mon => "segunda-feira",
        Weekday::Tue => "terça-feira",
        Weekday::Wed => "quarta-feira",
        Weekday::Thu => "quinta-feira",
        Weekday::Fri => "sexta-feira",
        Weekday::Sat => "sábado",
        Weekday::Sun => "domingo",
    }
}

pub fn dia_semana_curto(dia: NaiveDate) -> &'static str {
    match dia.weekday() {
        Weekday::Mon => "seg",
        Weekday::Tue => "ter",
        Weekday::Wed => "qua",
        Weekday::Thu => "qui",
        Weekday::Fri => "sex",
        Weekday::Sat => "sáb",
        Weekday::Sun => "dom",
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn dia() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 20).unwrap()
    }

    fn entrada(hora: &str, texto: &str) -> Entrada {
        Entrada {
            hora: hora.into(),
            texto: texto.into(),
        }
    }

    fn pasta_temporaria(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("diario-teste-{nome}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&pasta);
        pasta
    }

    #[test]
    fn escrever_e_ler_devolvem_as_mesmas_entradas() {
        let entradas = vec![
            entrada("09:15", "Revisei o PR.\nSegunda linha."),
            entrada("14:30", "Reunião com o suporte."),
        ];
        let texto = escrever(dia(), &entradas);
        assert!(texto.starts_with("# Diário de 20/11/2026 (sexta-feira)\n"));
        assert_eq!(ler(&texto), entradas);
    }

    #[test]
    fn texto_sem_cabecalho_nao_se_perde() {
        let lidas = ler("# Diário\n\nanotei à mão\n\n## 10:00\noutra coisa\n");
        assert_eq!(
            lidas,
            vec![entrada(SEM_HORA, "anotei à mão"), entrada("10:00", "outra coisa")]
        );
        assert_eq!(ler(&escrever(dia(), &lidas)), lidas);
    }

    #[test]
    fn salvar_carregar_e_apagar_o_dia() {
        let diario = Diario::new(pasta_temporaria("salvar"));
        assert!(diario.carregar(dia()).unwrap().is_empty());

        let entradas = vec![entrada("08:00", "Comecei o dia.")];
        diario.salvar(dia(), &entradas).unwrap();
        assert_eq!(diario.carregar(dia()).unwrap(), entradas);
        assert_eq!(diario.dias_com_anotacoes(), vec![dia()]);

        diario.salvar(dia(), &[]).unwrap();
        assert!(!diario.arquivo(dia()).exists());
        assert!(diario.dias_com_anotacoes().is_empty());
        let _ = fs::remove_dir_all(diario.pasta());
    }

    #[test]
    fn texto_para_daily_vira_lista() {
        let hoje = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        let texto = texto_para_daily(dia(), hoje, &[entrada("09:00", "Fiz  a\ntarefa"), entrada("10:00", "Deploy")]);
        assert_eq!(texto, "Ontem (20/11):\n- Fiz a tarefa\n- Deploy\n");
    }

    #[test]
    fn nomes_relativos() {
        let hoje = dia();
        assert_eq!(nome_relativo(hoje, hoje), "Hoje");
        assert_eq!(nome_relativo(hoje.pred_opt().unwrap(), hoje), "Ontem");
        assert_eq!(nome_relativo(NaiveDate::from_ymd_opt(2026, 11, 17).unwrap(), hoje), "Terça-feira");
    }
}
