//! Load the weights, vocabulary and preprocessing parameters from the same GGUF
//! selected and checksum-verified by Kar. Candle uses Hugging Face tensor names.
use std::{collections::HashMap, fs::File, io::BufReader, path::Path};

use ahash::AHashMap;
use anyhow::{Context, Result, anyhow, ensure};
use candle_core::{
    DType, Device, Tensor,
    quantized::gguf_file::{Content, Value},
};
use candle_nn::VarBuilder;
use candle_transformers::models::whisper::{Config, model::Whisper};
use tokenizers::{Tokenizer, models::bpe::BPE, pre_tokenizers::byte_level::ByteLevel};

use crate::{Recognizer, audio::Frontend};

struct Metadata<'a>(&'a HashMap<String, Value>);

impl Metadata<'_> {
    fn get(&self, key: &str) -> Result<&Value> {
        self.0
            .get(key)
            .with_context(|| format!("missing GGUF metadata {key}"))
    }

    fn integer(&self, key: &str) -> Result<usize> {
        let v = self.get(key)?;
        Ok(usize::try_from(match v {
            Value::I32(n) => u64::try_from(*n)?,
            _ => v.to_u64()?,
        })?)
    }

    fn strings(&self, key: &str) -> Result<Vec<String>> {
        self.get(key)?
            .to_vec()?
            .iter()
            .map(|s| Ok(s.to_string()?.clone()))
            .collect()
    }

    fn ids(&self, key: &str) -> Result<Vec<u32>> {
        self.get(key)?
            .to_vec()?
            .iter()
            .map(|v| match v {
                Value::I32(n) => Ok(u32::try_from(*n)?),
                _ => Ok(u32::try_from(v.to_u64()?)?),
            })
            .collect()
    }
}

fn tokenizer(meta: &Metadata<'_>) -> Result<Tokenizer> {
    let vocab: AHashMap<_, _> = meta
        .strings("tokenizer.ggml.tokens")?
        .into_iter()
        .enumerate()
        .map(|(i, token)| (token, i as u32))
        .collect();
    let merges = meta
        .strings("tokenizer.ggml.merges")?
        .into_iter()
        .map(|s| {
            let (left, right) = s.split_once(' ').context("invalid GGUF tokenizer merge")?;
            Ok((left.to_owned(), right.to_owned()))
        })
        .collect::<Result<Vec<_>>>()?;
    let bpe = BPE::builder()
        .vocab_and_merges(vocab, merges)
        .build()
        .map_err(|e| anyhow!("{e}"))?;
    let bytes = ByteLevel::new(false, false, true);
    let mut tokenizer = Tokenizer::new(bpe);
    tokenizer.with_pre_tokenizer(Some(bytes));
    tokenizer.with_decoder(Some(bytes));
    Ok(tokenizer)
}

fn candle_name(gguf_name: &str) -> Option<String> {
    let (stack, name) = gguf_name.split_once('.')?;
    let prefix = match stack {
        "enc" => "model.encoder",
        "dec" => "model.decoder",
        _ => return None,
    };
    // Candle generates the fixed encoder sinusoidal positions internally.
    if stack == "enc" && name == "pos_emb.weight" {
        return None;
    }
    let name = name
        .replace("conv.0.", "conv1.")
        .replace("conv.1.", "conv2.")
        .replace("token_embd.", "embed_tokens.")
        .replace("pos_emb.", "embed_positions.")
        .replace("final_norm.", "layer_norm.")
        .replace("blocks.", "layers.")
        .replace(".norm_attn.", ".self_attn_layer_norm.")
        .replace(".norm_self.", ".self_attn_layer_norm.")
        .replace(".norm_cross.", ".encoder_attn_layer_norm.")
        .replace(".norm_ffn.", ".final_layer_norm.")
        .replace(".attn.", ".self_attn.")
        .replace(".cross_attn.", ".encoder_attn.")
        .replace(".ffn.", ".")
        .replace(".q.", ".q_proj.")
        .replace(".k.", ".k_proj.")
        .replace(".v.", ".v_proj.")
        .replace(".out.", ".out_proj.");
    Some(format!("{prefix}.{name}"))
}

impl Recognizer {
    pub fn load(path: &Path) -> Result<Self> {
        let mut file = BufReader::new(File::open(path)?);
        let gguf = Content::read(&mut file)?;
        let meta = Metadata(&gguf.metadata);
        ensure!(
            meta.get("general.architecture")?.to_string()? == "whisper",
            "expected a Whisper GGUF model"
        );
        let cfg = Config {
            num_mel_bins: meta.integer("stt.whisper.encoder.num_mel_bins")?,
            max_source_positions: meta.integer("stt.whisper.encoder.max_source_positions")?,
            d_model: meta.integer("stt.whisper.encoder.d_model")?,
            encoder_attention_heads: meta.integer("stt.whisper.encoder.n_heads")?,
            encoder_layers: meta.integer("stt.whisper.encoder.n_layers")?,
            vocab_size: meta.integer("stt.whisper.decoder.vocab_size")?,
            max_target_positions: meta.integer("stt.whisper.decoder.max_target_positions")?,
            decoder_attention_heads: meta.integer("stt.whisper.decoder.n_heads")?,
            decoder_layers: meta.integer("stt.whisper.decoder.n_layers")?,
            suppress_tokens: meta.ids("stt.whisper.suppress_tokens")?,
        };
        let mut tensor = |key: &str| -> Result<Tensor> {
            Ok(gguf
                .tensor(&mut file, key, &Device::Cpu)?
                .dequantize(&Device::Cpu)?)
        };
        let filters = tensor("frontend.mel_filterbank")?;
        let window = tensor("frontend.window")?.to_vec1::<f32>()?;
        let frontend = Frontend::new(
            filters,
            window,
            cfg.num_mel_bins,
            cfg.max_source_positions * 2,
            meta.integer("stt.frontend.sample_rate")?,
            meta.integer("stt.frontend.hop_length")?,
        )?;
        let mut weights = HashMap::new();
        for key in gguf.tensor_infos.keys() {
            if let Some(name) = candle_name(key) {
                weights.insert(name, tensor(key)?);
            }
        }
        let whisper = Whisper::load(
            &VarBuilder::from_tensors(weights, DType::F32, &Device::Cpu),
            cfg,
        )?;
        let tokenizer = tokenizer(&meta)?;
        let begin_suppress = meta.ids("stt.whisper.begin_suppress_tokens")?;
        let languages = meta.strings("general.languages")?;
        Recognizer::new(whisper, frontend, tokenizer, begin_suppress, languages)
    }
}
