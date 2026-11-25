//! Diário de trabalho por voz.
//!
//! - `audio`: grava o microfone e entrega áudio mono a 16 kHz.
//! - `transcricao`: transforma o áudio em texto com o Whisper, offline.
//! - `modelo`: baixa o modelo do Whisper na primeira execução.
//! - `armazenamento`: guarda as anotações em um arquivo Markdown por dia.

pub mod armazenamento;
pub mod audio;
pub mod modelo;
pub mod transcricao;

use std::path::PathBuf;

/// Pasta onde ficam as anotações, o modelo e o estado do app.
///
/// Padrão: `%LOCALAPPDATA%\Diario`. A variável `DIARIO_DIR` troca o local.
pub fn pasta_base() -> PathBuf {
    if let Some(pasta) = std::env::var_os("DIARIO_DIR") {
        return PathBuf::from(pasta);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Diario")
}
