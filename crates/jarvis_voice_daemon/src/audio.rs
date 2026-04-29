//! Audio capture y playback con cpal.
//!
//! Captura mono 16kHz int16 desde el default input device de cpal, que
//! en Arch + WirePlumber se mapea automáticamente a la fuente de
//! PipeWire. Los chunks salen por un canal MPSC hacia el orquestador.
//!
//! Playback: el orquestador recibe PCM 16kHz int16 desde el server y lo
//! mete en un ringbuf. El callback de cpal lee del ringbuf y reproduce.
//! Para barge-in, el orquestador limpia el ringbuf cuando llega un
//! evento `interruption`.

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{
    HeapRb,
    traits::{Consumer, Producer, Split},
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;

/// Tamaño de chunk: ~50ms a 16kHz = 800 samples = 1600 bytes int16.
/// ElevenLabs recomienda chunks ~50-100ms para latencia óptima.
pub const CHUNK_SAMPLES: usize = 800;
pub const SAMPLE_RATE: u32 = 16_000;

/// Capacidad del ringbuf de playback: 16kHz × 2s = 32000 samples (~64KB).
const PLAYBACK_BUFFER_SAMPLES: usize = 32_000;

pub struct AudioIo {
    pub mic_rx: mpsc::Receiver<Vec<i16>>,
    pub speaker_tx: SpeakerTx,
    /// Mantiene los streams vivos. Al dropear esto el callback se para.
    _input_stream: cpal::Stream,
    _output_stream: cpal::Stream,
}

/// Handler que el orquestador usa para enviar audio al speaker.
#[derive(Clone)]
pub struct SpeakerTx {
    tx: mpsc::UnboundedSender<SpeakerCmd>,
    /// Bandera "limpiar buffer" que el callback de output respeta.
    flush_flag: Arc<AtomicBool>,
}

enum SpeakerCmd {
    Pcm(Vec<i16>),
}

impl SpeakerTx {
    pub fn play(&self, pcm: Vec<i16>) {
        let _ = self.tx.send(SpeakerCmd::Pcm(pcm));
    }

    /// Pide al callback de output que descarte el audio en buffer.
    /// Usado para barge-in cuando el server envía `interruption`.
    pub fn flush(&self) {
        self.flush_flag.store(true, Ordering::Release);
    }
}

pub fn start() -> Result<AudioIo> {
    let host = cpal::default_host();

    // ─── Input (mic) ───
    let input_device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("no default audio input device"))?;
    let input_config = input_device
        .default_input_config()
        .context("getting default input config")?;
    tracing::info!(
        sample_rate = input_config.sample_rate().0,
        channels = input_config.channels(),
        format = ?input_config.sample_format(),
        device = %input_device.name().unwrap_or_else(|_| "<unknown>".into()),
        "audio.input_device"
    );

    let (mic_tx, mic_rx) = mpsc::channel::<Vec<i16>>(64);
    let input_stream = build_input_stream(&input_device, &input_config, mic_tx)?;
    input_stream.play().context("starting input stream")?;

    // ─── Output (speaker) ───
    let output_device = host
        .default_output_device()
        .ok_or_else(|| anyhow!("no default audio output device"))?;
    let output_config = output_device
        .default_output_config()
        .context("getting default output config")?;
    tracing::info!(
        sample_rate = output_config.sample_rate().0,
        channels = output_config.channels(),
        format = ?output_config.sample_format(),
        device = %output_device.name().unwrap_or_else(|_| "<unknown>".into()),
        "audio.output_device"
    );

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<SpeakerCmd>();
    let flush_flag = Arc::new(AtomicBool::new(false));
    let output_stream =
        build_output_stream(&output_device, &output_config, cmd_rx, flush_flag.clone())?;
    output_stream.play().context("starting output stream")?;

    Ok(AudioIo {
        mic_rx,
        speaker_tx: SpeakerTx {
            tx: cmd_tx,
            flush_flag,
        },
        _input_stream: input_stream,
        _output_stream: output_stream,
    })
}

fn build_input_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    tx: mpsc::Sender<Vec<i16>>,
) -> Result<cpal::Stream> {
    let stream_config: cpal::StreamConfig = config.config();
    let device_rate = stream_config.sample_rate.0 as f32;
    let device_channels = stream_config.channels as usize;
    let target_rate = SAMPLE_RATE as f32;
    // Resampler simple por decimación lineal: válido para STT, no para audiophile.
    // El caso normal en Linux es device_rate=48000 → ratio=3.0 (entero, exacto).
    let ratio = device_rate / target_rate;

    let mut chunk_buf: Vec<i16> = Vec::with_capacity(CHUNK_SAMPLES);
    let mut subsample_acc: f32 = 0.0;
    let err_fn = |e| tracing::warn!(error = %e, "audio.input_error");

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| {
                process_input::<f32>(
                    data,
                    device_channels,
                    ratio,
                    &mut subsample_acc,
                    &mut chunk_buf,
                    &tx,
                );
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| {
                process_input::<i16>(
                    data,
                    device_channels,
                    ratio,
                    &mut subsample_acc,
                    &mut chunk_buf,
                    &tx,
                );
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _| {
                process_input::<u16>(
                    data,
                    device_channels,
                    ratio,
                    &mut subsample_acc,
                    &mut chunk_buf,
                    &tx,
                );
            },
            err_fn,
            None,
        )?,
        other => return Err(anyhow!("unsupported input sample format: {other:?}")),
    };

    Ok(stream)
}

trait ToI16 {
    fn to_i16(self) -> i16;
}

impl ToI16 for f32 {
    fn to_i16(self) -> i16 {
        (self.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
    }
}
impl ToI16 for i16 {
    fn to_i16(self) -> i16 {
        self
    }
}
impl ToI16 for u16 {
    fn to_i16(self) -> i16 {
        (self as i32 - i16::MAX as i32) as i16
    }
}

/// Mezcla canales (toma sólo el canal 0 si hay varios), submuestrea al
/// rate target, y emite chunks de CHUNK_SAMPLES samples por el canal.
fn process_input<S: Copy + ToI16>(
    data: &[S],
    channels: usize,
    ratio: f32,
    subsample_acc: &mut f32,
    chunk_buf: &mut Vec<i16>,
    tx: &mpsc::Sender<Vec<i16>>,
) {
    let mut i = 0;
    while i < data.len() {
        // Avanza al siguiente sample que toca según el ratio.
        if *subsample_acc < 1.0 {
            // Toma el sample actual (canal 0 sólo).
            chunk_buf.push(data[i].to_i16());
            *subsample_acc += ratio;
        }
        *subsample_acc -= 1.0;
        i += channels;

        if chunk_buf.len() >= CHUNK_SAMPLES {
            let chunk = std::mem::replace(chunk_buf, Vec::with_capacity(CHUNK_SAMPLES));
            // try_send para no bloquear el hilo de audio si el consumer
            // está atrás. Si full, drop del frame.
            if tx.try_send(chunk).is_err() {
                tracing::warn!("audio.mic_queue_full_dropping");
            }
        }
    }
}

fn build_output_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    mut cmd_rx: mpsc::UnboundedReceiver<SpeakerCmd>,
    flush_flag: Arc<AtomicBool>,
) -> Result<cpal::Stream> {
    let stream_config: cpal::StreamConfig = config.config();
    let device_rate = stream_config.sample_rate.0 as f32;
    let device_channels = stream_config.channels as usize;
    let source_rate = SAMPLE_RATE as f32;

    // Ringbuf compartido: producer en el reader del canal, consumer en
    // el callback de cpal. Operaciones lock-free.
    let rb = HeapRb::<i16>::new(PLAYBACK_BUFFER_SAMPLES);
    let (mut prod, mut cons) = rb.split();

    // Hilo dedicado para drenar cmd_rx → ringbuf. Usamos un thread
    // bloqueante porque cpal no es async.
    std::thread::spawn(move || {
        // Resample lineal de 16kHz → device_rate. Mantén un sample
        // anterior para interpolación.
        let ratio_in_per_out = source_rate / device_rate;
        let mut prev: i16 = 0;
        let mut frac: f32 = 0.0;

        loop {
            let cmd = match cmd_rx.blocking_recv() {
                Some(c) => c,
                None => break, // canal cerrado → termina hilo
            };
            let SpeakerCmd::Pcm(pcm) = cmd;

            let mut idx_f: f32 = frac;
            let mut iter = pcm.into_iter();
            let mut current: Option<i16> = iter.next();
            while let Some(cur) = current {
                while idx_f < 1.0 {
                    // Interpolación lineal entre prev y cur.
                    let interp = prev as f32 * (1.0 - idx_f) + cur as f32 * idx_f;
                    let sample = interp as i16;
                    // Duplicar a todos los canales del device.
                    for _ in 0..device_channels {
                        if prod.try_push(sample).is_err() {
                            // Ringbuf lleno → caemos al callback que
                            // probablemente se está saltando frames; loseamos
                            // este sample.
                            break;
                        }
                    }
                    idx_f += ratio_in_per_out;
                }
                idx_f -= 1.0;
                prev = cur;
                current = iter.next();
            }
            frac = idx_f;
        }
    });

    let err_fn = |e| tracing::warn!(error = %e, "audio.output_error");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &stream_config,
            move |out: &mut [f32], _| {
                if flush_flag.swap(false, Ordering::AcqRel) {
                    drain_ringbuf(&mut cons);
                }
                for slot in out.iter_mut() {
                    *slot = match cons.try_pop() {
                        Some(s) => s as f32 / i16::MAX as f32,
                        None => 0.0,
                    };
                }
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_output_stream(
            &stream_config,
            move |out: &mut [i16], _| {
                if flush_flag.swap(false, Ordering::AcqRel) {
                    drain_ringbuf(&mut cons);
                }
                for slot in out.iter_mut() {
                    *slot = cons.try_pop().unwrap_or(0);
                }
            },
            err_fn,
            None,
        )?,
        other => return Err(anyhow!("unsupported output sample format: {other:?}")),
    };

    Ok(stream)
}

fn drain_ringbuf(cons: &mut impl Consumer<Item = i16>) {
    while cons.try_pop().is_some() {}
}
