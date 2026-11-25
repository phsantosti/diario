//! Transcreve um arquivo WAV pela linha de comando, para testar o modelo e medir o tempo.
//!
//! `cargo run --release --example transcrever -- caminho.wav [threads] [feixe]`

use diario::transcricao::Transcritor;
use diario::{audio, modelo, pasta_base};
use std::time::Instant;

fn main() -> Result<(), String> {
    let mut argumentos = std::env::args().skip(1);
    let wav = argumentos
        .next()
        .ok_or("uso: transcrever <arquivo.wav> [threads] [feixe]")?;
    let threads: Option<i32> = argumentos.next().and_then(|t| t.parse().ok());
    let feixe: Option<i32> = argumentos.next().and_then(|f| f.parse().ok());

    let caminho_modelo = modelo::caminho(&pasta_base());
    if !caminho_modelo.exists() {
        println!("baixando {} ...", modelo::nome());
        modelo::baixar(&modelo::url(&modelo::nome()), &caminho_modelo, |_, _| {})?;
    }

    let leitor = hound::WavReader::open(&wav).map_err(|e| format!("{wav}: {e}"))?;
    let spec = leitor.spec();
    let amostras: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => leitor.into_samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        hound::SampleFormat::Int => {
            let maximo = (1i64 << (spec.bits_per_sample - 1)) as f32;
            leitor
                .into_samples::<i32>()
                .map(|s| s.unwrap_or(0) as f32 / maximo)
                .collect()
        }
    };
    let canais = usize::from(spec.channels).max(1);
    let mono: Vec<f32> = amostras
        .chunks(canais)
        .map(|quadro| quadro.iter().sum::<f32>() / quadro.len() as f32)
        .collect();
    let audio = audio::reamostrar(&mono, spec.sample_rate, audio::TAXA_WHISPER);

    let inicio = Instant::now();
    let mut transcritor = Transcritor::carregar(&caminho_modelo)?;
    if let Some(threads) = threads {
        transcritor = transcritor.com_threads(threads);
    }
    if let Some(feixe) = feixe {
        transcritor = transcritor.com_feixe(feixe);
    }
    println!("modelo {} carregado em {:.1}s", modelo::nome(), inicio.elapsed().as_secs_f32());

    let inicio = Instant::now();
    let texto = transcritor.transcrever(&audio)?;
    println!(
        "áudio de {:.1}s transcrito em {:.1}s",
        audio.len() as f32 / audio::TAXA_WHISPER as f32,
        inicio.elapsed().as_secs_f32()
    );
    println!("{texto}");
    Ok(())
}
