//! Gravação do microfone. Entrega o áudio mono a 16 kHz, que é o que o Whisper espera.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const TAXA_WHISPER: u32 = 16_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Microfone {
    pub id: String,
    pub nome: String,
}

/// Microfones disponíveis no Windows.
pub fn listar_microfones() -> Vec<Microfone> {
    let host = cpal::default_host();
    let Ok(dispositivos) = host.input_devices() else {
        return Vec::new();
    };
    dispositivos
        .filter_map(|dispositivo| {
            let id = dispositivo.id().ok()?.to_string();
            let nome = dispositivo
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| id.clone());
            Some(Microfone { id, nome })
        })
        .collect()
}

/// Uma gravação em andamento. O stream do microfone vive numa thread própria
/// e é fechado quando `parar` é chamado (ou quando a gravação é descartada).
pub struct Gravacao {
    parar: mpsc::Sender<()>,
    thread: Option<JoinHandle<Result<Vec<f32>, String>>>,
    nivel: Arc<AtomicU32>,
    inicio: Instant,
}

impl Gravacao {
    /// Começa a gravar. `id_microfone` vazio usa o microfone padrão do Windows.
    pub fn iniciar(id_microfone: Option<&str>) -> Result<Self, String> {
        let (tx_pronto, rx_pronto) = mpsc::channel();
        let (tx_parar, rx_parar) = mpsc::channel();
        let nivel = Arc::new(AtomicU32::new(0));
        let id = id_microfone.map(str::to_owned);
        let nivel_thread = nivel.clone();
        let thread = std::thread::Builder::new()
            .name("microfone".into())
            .spawn(move || gravar(id, nivel_thread, tx_pronto, rx_parar))
            .map_err(|e| format!("Não consegui iniciar a gravação: {e}"))?;

        match rx_pronto.recv() {
            Ok(Ok(())) => Ok(Self {
                parar: tx_parar,
                thread: Some(thread),
                nivel,
                inicio: Instant::now(),
            }),
            Ok(Err(erro)) => {
                let _ = thread.join();
                Err(erro)
            }
            Err(_) => {
                let _ = thread.join();
                Err("A gravação parou inesperadamente.".into())
            }
        }
    }

    /// Volume de pico do último trecho, de 0 a 1.
    pub fn nivel(&self) -> f32 {
        f32::from_bits(self.nivel.load(Ordering::Relaxed))
    }

    pub fn duracao(&self) -> Duration {
        self.inicio.elapsed()
    }

    /// Para de gravar e devolve o áudio mono a 16 kHz.
    pub fn parar(mut self) -> Result<Vec<f32>, String> {
        let _ = self.parar.send(());
        match self.thread.take() {
            Some(thread) => thread
                .join()
                .map_err(|_| "A gravação parou inesperadamente.".to_string())?,
            None => Ok(Vec::new()),
        }
    }
}

impl Drop for Gravacao {
    fn drop(&mut self) {
        let _ = self.parar.send(());
    }
}

fn gravar(
    id: Option<String>,
    nivel: Arc<AtomicU32>,
    pronto: mpsc::Sender<Result<(), String>>,
    parar: mpsc::Receiver<()>,
) -> Result<Vec<f32>, String> {
    let (stream, amostras, taxa) = match abrir_microfone(id.as_deref(), nivel) {
        Ok(aberto) => aberto,
        Err(erro) => {
            let _ = pronto.send(Err(erro.clone()));
            return Err(erro);
        }
    };
    let _ = pronto.send(Ok(()));
    // Espera o pedido de parada (ou a Gravacao ser descartada).
    let _ = parar.recv();
    drop(stream);
    let bruto = std::mem::take(&mut *amostras.lock().map_err(|_| "Falha ao ler o áudio gravado.")?);
    Ok(reamostrar(&bruto, taxa, TAXA_WHISPER))
}

type Amostras = Arc<Mutex<Vec<f32>>>;

fn abrir_microfone(
    id: Option<&str>,
    nivel: Arc<AtomicU32>,
) -> Result<(cpal::Stream, Amostras, u32), String> {
    let host = cpal::default_host();
    let escolhido = id
        .and_then(|id| id.parse::<cpal::DeviceId>().ok())
        .and_then(|id| host.device_by_id(&id));
    // Se o microfone salvo sumiu (desplugado), cai no padrão do Windows.
    let dispositivo = escolhido
        .or_else(|| host.default_input_device())
        .ok_or("Nenhum microfone encontrado. Conecte um microfone e tente de novo.")?;

    let config = dispositivo
        .default_input_config()
        .map_err(|e| format!("Não consegui abrir o microfone: {e}"))?;
    let canais = usize::from(config.channels()).max(1);
    let taxa = config.sample_rate();
    let amostras: Amostras = Arc::new(Mutex::new(Vec::with_capacity(taxa as usize * 60)));
    let formato = config.sample_format();
    let config: cpal::StreamConfig = config.into();

    let stream = match formato {
        SampleFormat::F32 => construir::<f32>(&dispositivo, config, canais, amostras.clone(), nivel),
        SampleFormat::I16 => construir::<i16>(&dispositivo, config, canais, amostras.clone(), nivel),
        SampleFormat::I32 => construir::<i32>(&dispositivo, config, canais, amostras.clone(), nivel),
        SampleFormat::U16 => construir::<u16>(&dispositivo, config, canais, amostras.clone(), nivel),
        SampleFormat::U8 => construir::<u8>(&dispositivo, config, canais, amostras.clone(), nivel),
        SampleFormat::I8 => construir::<i8>(&dispositivo, config, canais, amostras.clone(), nivel),
        outro => return Err(format!("Formato de áudio do microfone não suportado: {outro}")),
    }?;
    stream
        .play()
        .map_err(|e| format!("Não consegui começar a gravar: {e}"))?;
    Ok((stream, amostras, taxa))
}

fn construir<T>(
    dispositivo: &cpal::Device,
    config: cpal::StreamConfig,
    canais: usize,
    amostras: Amostras,
    nivel: Arc<AtomicU32>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    dispositivo
        .build_input_stream(
            config,
            move |dados: &[T], _: &cpal::InputCallbackInfo| {
                let mut pico = 0f32;
                if let Ok(mut buffer) = amostras.lock() {
                    for quadro in dados.chunks(canais) {
                        let soma: f32 = quadro.iter().map(|&s| s.to_sample::<f32>()).sum();
                        let mono = soma / quadro.len() as f32;
                        pico = pico.max(mono.abs());
                        buffer.push(mono);
                    }
                }
                nivel.store(pico.to_bits(), Ordering::Relaxed);
            },
            |erro| eprintln!("erro no microfone: {erro}"),
            None,
        )
        .map_err(|e| format!("Não consegui abrir o microfone: {e}"))
}

/// Muda a taxa de amostragem. Ao reduzir (48 kHz → 16 kHz), tira a média da janela
/// de cada amostra de saída, o que já filtra o agudo que viraria ruído; ao aumentar,
/// interpola em linha reta.
pub fn reamostrar(entrada: &[f32], de: u32, para: u32) -> Vec<f32> {
    if de == para || entrada.is_empty() || de == 0 || para == 0 {
        return entrada.to_vec();
    }
    let razao = f64::from(de) / f64::from(para);
    let tamanho = (entrada.len() as f64 / razao).floor() as usize;
    let ultimo = entrada.len() - 1;

    (0..tamanho)
        .map(|i| {
            let centro = i as f64 * razao;
            if razao > 1.0 {
                let meia = razao / 2.0;
                let inicio = ((centro - meia).ceil().max(0.0) as usize).min(ultimo);
                let fim = ((centro + meia).floor() as usize).min(ultimo).max(inicio);
                let janela = &entrada[inicio..=fim];
                janela.iter().sum::<f32>() / janela.len() as f32
            } else {
                let base = (centro.floor() as usize).min(ultimo);
                let proximo = (base + 1).min(ultimo);
                let fracao = (centro - base as f64) as f32;
                entrada[base] + (entrada[proximo] - entrada[base]) * fracao
            }
        })
        .collect()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn reduz_48k_para_16k() {
        let entrada = vec![0.5f32; 48_000];
        let saida = reamostrar(&entrada, 48_000, 16_000);
        assert_eq!(saida.len(), 16_000);
        assert!(saida.iter().all(|&s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn reduz_44k1_para_16k() {
        let entrada = vec![0.25f32; 44_100];
        let saida = reamostrar(&entrada, 44_100, 16_000);
        assert_eq!(saida.len(), 16_000);
        assert!(saida.iter().all(|&s| (s - 0.25).abs() < 1e-6));
    }

    #[test]
    fn aumenta_8k_para_16k_interpolando() {
        let saida = reamostrar(&[0.0, 1.0, 0.0], 8_000, 16_000);
        assert_eq!(saida, vec![0.0, 0.5, 1.0, 0.5, 0.0, 0.0]);
    }

    #[test]
    fn mesma_taxa_devolve_igual() {
        assert_eq!(reamostrar(&[0.1, 0.2], 16_000, 16_000), vec![0.1, 0.2]);
    }
}
