//! Local `nvidia/Nemotron-3-Embed-1B-BF16` inference with candle (Metal on Apple Silicon, CPU
//! otherwise). The architecture is Ministral3 with *bidirectional* attention, YARN RoPE and mean
//! pooling; it is re-implemented here from the published tensors only. No repository code is
//! downloaded or executed, and weights are verified against pinned digests before use.
use super::{EmbedError, EmbeddingIdentity, EmbeddingProvider, normalize};
use crate::store::integrity::sha256;
use candle_core::{D, DType, Device, Module, Tensor};
use candle_nn::{Embedding, Linear, RmsNorm, VarBuilder, ops, rotary_emb};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

pub const MODEL_ID: &str = "nvidia/Nemotron-3-Embed-1B-BF16";
pub const REVISION: &str = "c0c9fea93ea424587517f2c59e20db9f1d6bf615";
const WEIGHTS_SHA256: &str = "f959c3b04e66b42de280bfb97c140cb7e0bfe25e3ecb0b4464c68a8436b2d04f";
const TOKENIZER_SHA256: &str = "797410dfb649a5b9ba92bc4fef7dbf4022d00e73de6867c4ac199a8846439421";
const CONFIG_SHA256: &str = "56964141409866da7ce7d2d88b5b20c44558f5e916b318ad3abd2c694b2cb5f7";
pub const QUERY_PREFIX: &str = "query: ";
pub const DOCUMENT_PREFIX: &str = "passage: ";
const PAD_TOKEN: u32 = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceChoice {
    Auto,
    Metal,
    Cpu,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    F32,
    F16,
    Bf16,
}
impl Precision {
    fn dtype(self) -> DType {
        match self {
            Self::F32 => DType::F32,
            Self::F16 => DType::F16,
            Self::Bf16 => DType::BF16,
        }
    }
}
#[derive(Debug, Clone)]
pub struct LocalConfig {
    pub model_dir: PathBuf,
    pub device: DeviceChoice,
    pub precision: Precision,
    /// Matryoshka slice (re-normalized). 1024 loses no recall on the development sets and halves storage.
    pub dimensions: usize,
    pub max_tokens: usize,
    /// Upper bound on padded tokens per forward pass.
    pub batch_tokens: usize,
}
impl Default for LocalConfig {
    fn default() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        Self {
            model_dir: home
                .join("Library/Application Support/PrimeRadiant/models/nemotron-3-embed-1b"),
            device: DeviceChoice::Auto,
            precision: Precision::F16,
            dimensions: 1024,
            max_tokens: 512,
            batch_tokens: 4096,
        }
    }
}
impl LocalConfig {
    pub fn identity(&self) -> EmbeddingIdentity {
        EmbeddingIdentity::new(
            "local",
            MODEL_ID,
            REVISION,
            self.dimensions,
            QUERY_PREFIX,
            DOCUMENT_PREFIX,
            self.max_tokens,
            true,
        )
    }
}

struct Cfg {
    hidden: usize,
    heads: usize,
    kv_heads: usize,
    head_dim: usize,
    layers: usize,
    eps: f64,
    inv_freq: Vec<f32>,
    attention_factor: f32,
    scale_beta: f64,
    scale_original: f64,
}
fn num(v: &Value, key: &str) -> Result<f64, EmbedError> {
    v.get(key)
        .and_then(Value::as_f64)
        .ok_or(EmbedError::Unavailable)
}
impl Cfg {
    fn parse(raw: &str) -> Result<Self, EmbedError> {
        let v: Value = serde_json::from_str(raw).map_err(|_| EmbedError::Unavailable)?;
        let rope = v.get("rope_parameters").ok_or(EmbedError::Unavailable)?;
        let head_dim = num(&v, "head_dim")? as usize;
        let base = num(rope, "rope_theta")?;
        let factor = num(rope, "factor")?;
        let original = num(rope, "original_max_position_embeddings")?;
        let (beta_fast, beta_slow) = (num(rope, "beta_fast")?, num(rope, "beta_slow")?);
        // YARN, as in the reference implementation (truncate=true): blend interpolation and
        // extrapolation per frequency band.
        let correction = |rotations: f64| {
            head_dim as f64 * (original / (rotations * 2.0 * std::f64::consts::PI)).ln()
                / (2.0 * base.ln())
        };
        let low = correction(beta_fast).floor().max(0.0);
        let high = correction(beta_slow).ceil().min(head_dim as f64 - 1.0);
        let high = if low == high { high + 0.001 } else { high };
        let inv_freq = (0..head_dim / 2)
            .map(|i| {
                let pos_freq = base.powf((2 * i) as f64 / head_dim as f64);
                let ramp = ((i as f64 - low) / (high - low)).clamp(0.0, 1.0);
                let extrapolation = 1.0 - ramp;
                ((1.0 / (factor * pos_freq)) * (1.0 - extrapolation)
                    + (1.0 / pos_freq) * extrapolation) as f32
            })
            .collect();
        let mscale = |s: f64, m: f64| {
            if s <= 1.0 {
                1.0
            } else {
                0.1 * m * s.ln() + 1.0
            }
        };
        let (m1, m2) = (
            rope.get("mscale").and_then(Value::as_f64),
            rope.get("mscale_all_dim").and_then(Value::as_f64),
        );
        let attention_factor = match (m1, m2) {
            (Some(a), Some(b)) if a != 0.0 && b != 0.0 => mscale(factor, a) / mscale(factor, b),
            _ => mscale(factor, 1.0),
        } as f32;
        Ok(Self {
            hidden: num(&v, "hidden_size")? as usize,
            heads: num(&v, "num_attention_heads")? as usize,
            kv_heads: num(&v, "num_key_value_heads")? as usize,
            head_dim,
            layers: num(&v, "num_hidden_layers")? as usize,
            eps: num(&v, "rms_norm_eps")?,
            inv_freq,
            attention_factor,
            scale_beta: num(rope, "llama_4_scaling_beta")?,
            scale_original: original,
        })
    }
}

struct Layer {
    q: Linear,
    k: Linear,
    v: Linear,
    o: Linear,
    gate: Linear,
    up: Linear,
    down: Linear,
    ln1: RmsNorm,
    ln2: RmsNorm,
}
struct Encoder {
    cfg: Cfg,
    embed: Embedding,
    layers: Vec<Layer>,
    norm: RmsNorm,
    device: Device,
    dtype: DType,
    tokenizer: tokenizers::Tokenizer,
}
fn linear(vb: &VarBuilder, name: &str, out: usize, inp: usize) -> candle_core::Result<Linear> {
    Ok(Linear::new(vb.pp(name).get((out, inp), "weight")?, None))
}
fn rms(vb: &VarBuilder, name: &str, size: usize, eps: f64) -> candle_core::Result<RmsNorm> {
    Ok(RmsNorm::new(vb.pp(name).get(size, "weight")?, eps))
}
fn read_verified(path: &Path, digest: &str) -> Result<Vec<u8>, EmbedError> {
    let bytes = std::fs::read(path).map_err(|_| EmbedError::Unavailable)?;
    if sha256(&bytes) != digest {
        return Err(EmbedError::Unavailable);
    }
    Ok(bytes)
}
impl Encoder {
    fn load(config: &LocalConfig) -> Result<Self, EmbedError> {
        let dir = &config.model_dir;
        let cfg = Cfg::parse(
            &String::from_utf8(read_verified(&dir.join("config.json"), CONFIG_SHA256)?)
                .map_err(|_| EmbedError::Unavailable)?,
        )?;
        let tokenizer = tokenizers::Tokenizer::from_bytes(read_verified(
            &dir.join("tokenizer.json"),
            TOKENIZER_SHA256,
        )?)
        .map_err(|_| EmbedError::Unavailable)?;
        let device = match config.device {
            DeviceChoice::Cpu => Device::Cpu,
            DeviceChoice::Metal => Device::new_metal(0).map_err(|_| EmbedError::Unavailable)?,
            DeviceChoice::Auto => Device::new_metal(0).unwrap_or(Device::Cpu),
        };
        let dtype = if device.is_cpu() && config.precision == Precision::F16 {
            DType::F32 // CPU half-precision matmul is slower than f32 and less exact
        } else {
            config.precision.dtype()
        };
        let weights = read_verified(&dir.join("model.safetensors"), WEIGHTS_SHA256)?;
        let vb = VarBuilder::from_buffered_safetensors(weights, dtype, &device)
            .map_err(|_| EmbedError::Unavailable)?;
        Self::build(cfg, &vb, device, dtype, tokenizer).map_err(|_| EmbedError::Unavailable)
    }
    fn build(
        cfg: Cfg,
        vb: &VarBuilder,
        device: Device,
        dtype: DType,
        tokenizer: tokenizers::Tokenizer,
    ) -> candle_core::Result<Self> {
        let embed = Embedding::new(
            vb.pp("embed_tokens").get((131072, cfg.hidden), "weight")?,
            cfg.hidden,
        );
        let (h, qd, kd) = (
            cfg.hidden,
            cfg.heads * cfg.head_dim,
            cfg.kv_heads * cfg.head_dim,
        );
        let inter = 6144;
        let mut layers = Vec::with_capacity(cfg.layers);
        for i in 0..cfg.layers {
            let l = vb.pp(format!("layers.{i}"));
            let a = l.pp("self_attn");
            let m = l.pp("mlp");
            layers.push(Layer {
                q: linear(&a, "q_proj", qd, h)?,
                k: linear(&a, "k_proj", kd, h)?,
                v: linear(&a, "v_proj", kd, h)?,
                o: linear(&a, "o_proj", h, qd)?,
                gate: linear(&m, "gate_proj", inter, h)?,
                up: linear(&m, "up_proj", inter, h)?,
                down: linear(&m, "down_proj", h, inter)?,
                ln1: rms(&l, "input_layernorm", h, cfg.eps)?,
                ln2: rms(&l, "post_attention_layernorm", h, cfg.eps)?,
            });
        }
        let norm = rms(vb, "norm", h, cfg.eps)?;
        Ok(Self {
            cfg,
            embed,
            layers,
            norm,
            device,
            dtype,
            tokenizer,
        })
    }

    /// `ids`/`mask`: [batch, len]; mask is 1 for real tokens (right padded). Returns the
    /// normalised-layer hidden states [batch, len, hidden].
    fn forward(&self, ids: &Tensor, mask: &Tensor) -> candle_core::Result<Tensor> {
        let (b, l) = ids.dims2()?;
        let c = &self.cfg;
        let mut x = self.embed.forward(ids)?;
        // Rotary tables for positions 0..l (right padding keeps real positions aligned).
        let freqs = Tensor::from_vec(c.inv_freq.clone(), (1, c.head_dim / 2), &self.device)?;
        let pos = Tensor::arange(0u32, l as u32, &self.device)?
            .to_dtype(DType::F32)?
            .reshape((l, 1))?;
        let angles = pos.matmul(&freqs)?;
        let cos = (angles.cos()? * c.attention_factor as f64)?.to_dtype(self.dtype)?;
        let sin = (angles.sin()? * c.attention_factor as f64)?.to_dtype(self.dtype)?;
        // Position-dependent query scale (identity below original_max_position_embeddings).
        let scale: Vec<f32> = (0..l)
            .map(|p| {
                (1.0 + c.scale_beta * (1.0 + (p as f64 / c.scale_original).floor()).ln()) as f32
            })
            .collect();
        let scale = Tensor::from_vec(scale, (1, 1, l, 1), &self.device)?.to_dtype(self.dtype)?;
        // Additive key mask: bidirectional attention over real tokens only.
        let neg = mask
            .to_dtype(DType::F32)?
            .affine(1e9, -1e9)?
            .reshape((b, 1, 1, l))?;
        let rep = c.heads / c.kv_heads;
        for layer in &self.layers {
            let h = layer.ln1.forward(&x)?;
            let shape = |t: Tensor, n: usize| -> candle_core::Result<Tensor> {
                t.reshape((b, l, n, c.head_dim))?
                    .transpose(1, 2)?
                    .contiguous()
            };
            let q = shape(layer.q.forward(&h)?, c.heads)?;
            let k = shape(layer.k.forward(&h)?, c.kv_heads)?;
            let v = shape(layer.v.forward(&h)?, c.kv_heads)?;
            let q = rotary_emb::rope(&q, &cos, &sin)?;
            let k = rotary_emb::rope(&k, &cos, &sin)?;
            let q = q.broadcast_mul(&scale)?;
            let expand = |t: Tensor| -> candle_core::Result<Tensor> {
                if rep == 1 {
                    return Ok(t);
                }
                let t = t.unsqueeze(2)?;
                Tensor::cat(&vec![&t; rep], 2)?.reshape((b, c.heads, l, c.head_dim))
            };
            let (k, v) = (expand(k)?, expand(v)?);
            let scores = (q.matmul(&k.transpose(2, 3)?.contiguous()?)?
                * (c.head_dim as f64).powf(-0.5))?
            .to_dtype(DType::F32)?
            .broadcast_add(&neg)?;
            let probs = ops::softmax_last_dim(&scores)?.to_dtype(self.dtype)?;
            let attn = probs.matmul(&v)?.transpose(1, 2)?.contiguous()?.reshape((
                b,
                l,
                c.heads * c.head_dim,
            ))?;
            x = (x + layer.o.forward(&attn)?)?;
            let h = layer.ln2.forward(&x)?;
            let act = (ops::silu(&layer.gate.forward(&h)?)? * layer.up.forward(&h)?)?;
            x = (x + layer.down.forward(&act)?)?;
        }
        self.norm.forward(&x)
    }

    fn embed_batch(
        &self,
        ids: &[Vec<u32>],
        dimensions: usize,
    ) -> candle_core::Result<Vec<Vec<f32>>> {
        let len = ids.iter().map(Vec::len).max().unwrap_or(1);
        let mut flat = Vec::with_capacity(ids.len() * len);
        let mut mask = Vec::with_capacity(ids.len() * len);
        for row in ids {
            flat.extend(row.iter().copied());
            flat.extend(std::iter::repeat_n(PAD_TOKEN, len - row.len()));
            mask.extend(std::iter::repeat_n(1u32, row.len()));
            mask.extend(std::iter::repeat_n(0u32, len - row.len()));
        }
        let ids_t = Tensor::from_vec(flat, (ids.len(), len), &self.device)?;
        let mask_t = Tensor::from_vec(mask, (ids.len(), len), &self.device)?;
        let hidden = self.forward(&ids_t, &mask_t)?.to_dtype(DType::F32)?;
        let m = mask_t.to_dtype(DType::F32)?.unsqueeze(D::Minus1)?;
        let pooled = hidden
            .broadcast_mul(&m)?
            .sum(1)?
            .broadcast_div(&m.sum(1)?)?;
        let rows = pooled.to_vec2::<f32>()?;
        Ok(rows
            .into_iter()
            .map(|mut row| {
                normalize(&mut row);
                row.truncate(dimensions);
                normalize(&mut row);
                row
            })
            .collect())
    }
}

/// Lazily loads the model on first use; one forward pass at a time.
pub struct LocalProvider {
    config: LocalConfig,
    identity: EmbeddingIdentity,
    encoder: Mutex<Option<Encoder>>,
    calls: AtomicU64,
}
impl LocalProvider {
    pub fn new(config: LocalConfig) -> Self {
        let identity = config.identity();
        Self {
            config,
            identity,
            encoder: Mutex::new(None),
            calls: AtomicU64::new(0),
        }
    }
    /// True when the pinned model files are present (digests are verified at first load).
    pub fn files_present(&self) -> bool {
        ["config.json", "tokenizer.json", "model.safetensors"]
            .iter()
            .all(|f| self.config.model_dir.join(f).is_file())
    }
}
impl EmbeddingProvider for LocalProvider {
    fn identity(&self) -> &EmbeddingIdentity {
        &self.identity
    }
    fn calls(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }
    fn embed_raw(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut guard = self.encoder.lock().map_err(|_| EmbedError::Runtime)?;
        if guard.is_none() {
            let started = std::time::Instant::now();
            *guard = Some(Encoder::load(&self.config)?);
            tracing::info!(
                seconds = started.elapsed().as_secs_f64(),
                "local embedding initialized"
            );
        }
        let enc = guard.as_ref().expect("loaded");
        let encodings = enc
            .tokenizer
            .encode_batch(texts.iter().map(String::as_str).collect::<Vec<_>>(), false)
            .map_err(|_| EmbedError::InvalidRequest)?;
        let mut ids: Vec<Vec<u32>> = encodings
            .iter()
            .map(|e| {
                let mut v = e.get_ids().to_vec();
                v.truncate(self.config.max_tokens);
                if v.is_empty() {
                    v.push(PAD_TOKEN);
                }
                v
            })
            .collect();
        // Length-sorted micro-batches bound padding waste; output order is restored below.
        let mut order: Vec<usize> = (0..ids.len()).collect();
        order.sort_by_key(|&i| ids[i].len());
        let mut out: Vec<Option<Vec<f32>>> = vec![None; ids.len()];
        let mut start = 0;
        while start < order.len() {
            let mut end = start + 1;
            while end < order.len()
                && end - start < 64
                && (end - start + 1) * ids[order[end]].len() <= self.config.batch_tokens
            {
                end += 1;
            }
            let batch: Vec<Vec<u32>> = order[start..end]
                .iter()
                .map(|&i| std::mem::take(&mut ids[i]))
                .collect();
            let vectors = enc
                .embed_batch(&batch, self.config.dimensions)
                .map_err(|_| EmbedError::Runtime)?;
            self.calls.fetch_add(1, Ordering::Relaxed);
            for (&i, v) in order[start..end].iter().zip(vectors) {
                out[i] = Some(v);
            }
            start = end;
        }
        out.into_iter()
            .map(|v| v.ok_or(EmbedError::Runtime))
            .collect()
    }
}
