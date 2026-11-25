//! Modelo do Whisper usado na transcrição. É baixado uma vez para `modelos/`.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

/// Bom equilíbrio entre acerto em português e velocidade no processador.
/// Troque com a variável `DIARIO_MODELO` (ex.: `ggml-large-v3-turbo-q5_0.bin`).
pub const MODELO_PADRAO: &str = "ggml-small.bin";

const ORIGEM: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Nome do arquivo do modelo escolhido.
pub fn nome() -> String {
    std::env::var("DIARIO_MODELO")
        .ok()
        .filter(|nome| nome_valido(nome))
        .unwrap_or_else(|| MODELO_PADRAO.to_string())
}

/// Só aceita nomes como `ggml-small.bin`, para a variável não virar caminho ou URL arbitrária.
fn nome_valido(nome: &str) -> bool {
    nome.starts_with("ggml-")
        && nome.ends_with(".bin")
        && nome.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !nome.contains("..")
}

pub fn caminho(pasta_base: &Path) -> PathBuf {
    pasta_base.join("modelos").join(nome())
}

pub fn url(nome: &str) -> String {
    format!("{ORIGEM}/{nome}")
}

/// Baixa o modelo para `destino`, chamando `progresso(baixado, total)` a cada pedaço.
/// Usa um arquivo `.parcial` e só renomeia no fim, então um download interrompido
/// nunca é confundido com um modelo pronto.
pub fn baixar(
    url: &str,
    destino: &Path,
    mut progresso: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let pasta = destino.parent().ok_or("Caminho do modelo inválido.")?;
    fs::create_dir_all(pasta).map_err(|e| format!("Não consegui criar {}: {e}", pasta.display()))?;
    let parcial = destino.with_extension("bin.parcial");

    let resposta = ureq::get(url)
        .call()
        .map_err(|e| format!("Não consegui baixar o modelo de voz ({url}): {e}"))?;
    let total = resposta.body().content_length();
    let mut leitor = resposta.into_body().into_with_config().reader();
    let mut arquivo = BufWriter::new(
        File::create(&parcial).map_err(|e| format!("Não consegui gravar o modelo: {e}"))?,
    );

    let mut buffer = vec![0u8; 256 * 1024];
    let mut baixado = 0u64;
    loop {
        let lidos = leitor
            .read(&mut buffer)
            .map_err(|e| format!("O download do modelo foi interrompido: {e}"))?;
        if lidos == 0 {
            break;
        }
        arquivo
            .write_all(&buffer[..lidos])
            .map_err(|e| format!("Não consegui gravar o modelo: {e}"))?;
        baixado += lidos as u64;
        progresso(baixado, total);
    }
    arquivo
        .flush()
        .map_err(|e| format!("Não consegui gravar o modelo: {e}"))?;
    drop(arquivo);

    if let Some(total) = total {
        if baixado != total {
            let _ = fs::remove_file(&parcial);
            return Err(format!(
                "O download do modelo veio incompleto ({baixado} de {total} bytes). Tente de novo."
            ));
        }
    }
    fs::rename(&parcial, destino).map_err(|e| format!("Não consegui salvar o modelo: {e}"))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn so_aceita_nomes_de_modelo() {
        assert!(nome_valido("ggml-small.bin"));
        assert!(nome_valido("ggml-large-v3-turbo-q5_0.bin"));
        assert!(!nome_valido("../../etc.bin"));
        assert!(!nome_valido("ggml-..\\x.bin"));
        assert!(!nome_valido("https://exemplo.com/ggml-x.bin"));
        assert!(!nome_valido("small.bin"));
    }
}
