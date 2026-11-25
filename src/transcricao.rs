//! Fala → texto com o Whisper (whisper.cpp), rodando na própria máquina.

use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::TAXA_WHISPER;

/// Dá ao Whisper o tom e o vocabulário de um diário de trabalho em português,
/// o que melhora pontuação e termos técnicos.
const CONTEXTO: &str = "Anotações do meu dia de trabalho. Hoje revisei o pull request, \
fiz o commit, atualizei a tarefa no Jira e participei da daily.";

/// Frases que o Whisper costuma inventar quando o áudio é silêncio ou ruído.
const ALUCINACOES: &[&str] = &[
    "amara.org",
    "legendas pela comunidade",
    "legenda adriana zanotto",
    "inscreva-se no canal",
    "obrigado por assistir",
    "obrigada por assistir",
];

const FEIXE_PADRAO: i32 = 5;

/// Pedaços com probabilidade de "não é fala" acima disto são descartados.
const LIMITE_SEM_FALA: f32 = 0.6;

pub struct Transcritor {
    contexto: WhisperContext,
    threads: i32,
    /// Tamanho do beam search; 1 usa a busca gulosa (mais rápida).
    feixe: i32,
}

impl Transcritor {
    pub fn carregar(modelo: &Path) -> Result<Self, String> {
        // Silencia o log do whisper.cpp no console.
        whisper_rs::install_logging_hooks();
        let caminho = modelo
            .to_str()
            .ok_or("O caminho do modelo tem caracteres não suportados.")?;
        let contexto = WhisperContext::new_with_params(caminho, WhisperContextParameters::default())
            .map_err(|e| format!("Não consegui carregar o modelo de voz {}: {e}", modelo.display()))?;
        Ok(Self {
            contexto,
            threads: threads_padrao(),
            feixe: FEIXE_PADRAO,
        })
    }

    pub fn com_threads(mut self, threads: i32) -> Self {
        self.threads = threads.max(1);
        self
    }

    pub fn com_feixe(mut self, feixe: i32) -> Self {
        self.feixe = feixe.max(1);
        self
    }

    /// Transcreve áudio mono a 16 kHz. Devolve texto vazio se não houver fala.
    pub fn transcrever(&self, audio: &[f32]) -> Result<String, String> {
        // O whisper.cpp recusa trechos com menos de 1 s; completa com silêncio.
        let minimo = TAXA_WHISPER as usize * 5 / 4;
        let mut audio = audio.to_vec();
        if audio.len() < minimo {
            audio.resize(minimo, 0.0);
        }

        let mut estado = self
            .contexto
            .create_state()
            .map_err(|e| format!("Falha ao preparar a transcrição: {e}"))?;

        let estrategia = if self.feixe > 1 {
            SamplingStrategy::BeamSearch {
                beam_size: self.feixe,
                patience: -1.0,
            }
        } else {
            SamplingStrategy::Greedy { best_of: 1 }
        };
        let mut parametros = FullParams::new(estrategia);
        parametros.set_language(Some("pt"));
        parametros.set_translate(false);
        parametros.set_n_threads(self.threads);
        parametros.set_no_context(true);
        parametros.set_suppress_nst(true);
        parametros.set_initial_prompt(CONTEXTO);
        parametros.set_print_special(false);
        parametros.set_print_progress(false);
        parametros.set_print_realtime(false);
        parametros.set_print_timestamps(false);

        estado
            .full(parametros, &audio)
            .map_err(|e| format!("Falha na transcrição: {e}"))?;

        let mut partes = Vec::new();
        for segmento in estado.as_iter() {
            if segmento.no_speech_probability() > LIMITE_SEM_FALA {
                continue;
            }
            let texto = segmento
                .to_str_lossy()
                .map_err(|e| format!("Falha ao ler o texto transcrito: {e}"))?;
            let texto = texto.trim();
            if !texto.is_empty() && !eh_alucinacao(texto) {
                partes.push(texto.to_string());
            }
        }
        Ok(partes.join(" "))
    }
}

fn eh_alucinacao(texto: &str) -> bool {
    let minusculo = texto.to_lowercase();
    ALUCINACOES.iter().any(|frase| minusculo.contains(frase))
}

/// Metade dos núcleos lógicos (os núcleos físicos, em CPUs com hyper-threading), até 16.
/// Acima disso o whisper.cpp quase não ganha velocidade.
fn threads_padrao() -> i32 {
    let logicos = std::thread::available_parallelism().map_or(4, |n| n.get());
    (logicos / 2).clamp(1, 16) as i32
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn reconhece_alucinacoes_comuns() {
        assert!(eh_alucinacao("Legendas pela comunidade Amara.org"));
        assert!(eh_alucinacao("Obrigado por assistir!"));
        assert!(!eh_alucinacao("Hoje corrigi o bug do relatório."));
    }
}
