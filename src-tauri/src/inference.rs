use std::num::NonZeroU32;

use anyhow::{Context, Result};
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel},
    sampling::LlamaSampler,
};

/// Core generation logic. Takes an already-loaded backend and model.
/// This is the function called on every chat message — the model stays in memory.
pub fn generate_with(
    backend: &LlamaBackend,
    model: &LlamaModel,
    prompt: &str,
    max_tokens: u32,
) -> Result<String> {
    let ctx_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(8192));
    let mut ctx = model
        .new_context(backend, ctx_params)
        .context("Failed to create inference context")?;

    let tokens = model
        .str_to_token(prompt, AddBos::Always)
        .context("Failed to tokenize prompt")?;

    let n_prompt = tokens.len() as i32;

    let mut batch = LlamaBatch::new(tokens.len() + max_tokens as usize, 1);
    for (i, &token) in tokens.iter().enumerate() {
        let is_last = i == tokens.len() - 1;
        batch
            .add(token, i as i32, &[0], is_last)
            .context("Failed to add token to batch")?;
    }
    ctx.decode(&mut batch)
        .context("Failed to run first forward pass")?;

    let mut sampler = LlamaSampler::greedy();
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut output = String::new();
    let mut n_cur = n_prompt;

    loop {
        let next_token = sampler.sample(&ctx, -1);
        sampler.accept(next_token);

        if model.is_eog_token(next_token) || n_cur >= n_prompt + max_tokens as i32 {
            break;
        }

        let piece = model
            .token_to_piece(next_token, &mut decoder, false, None)
            .context("Failed to decode token to string")?;
        output.push_str(&piece);

        if output.contains("<|eot_id|>") || output.contains("<|im_end|>") {
            output.truncate(
                output
                    .find("<|eot_id|>")
                    .or_else(|| output.find("<|im_end|>"))
                    .unwrap_or(output.len()),
            );
            break;
        }

        batch.clear();
        batch
            .add(next_token, n_cur, &[0], true)
            .context("Failed to add generated token to batch")?;
        n_cur += 1;

        ctx.decode(&mut batch)
            .context("Failed to run forward pass")?;
    }

    Ok(output)
}

/// Streaming version of generate_with.
/// Calls `on_token` with each text fragment as it is produced.
/// When this function returns the generation is complete.
pub fn generate_streaming<F>(
    backend: &LlamaBackend,
    model: &LlamaModel,
    prompt: &str,
    max_tokens: u32,
    mut on_token: F,
) -> Result<()>
where
    F: FnMut(String),
{
    let ctx_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(8192));
    let mut ctx = model
        .new_context(backend, ctx_params)
        .context("Failed to create inference context")?;

    let tokens = model
        .str_to_token(prompt, AddBos::Always)
        .context("Failed to tokenize prompt")?;

    let n_prompt = tokens.len() as i32;

    let mut batch = LlamaBatch::new(tokens.len() + max_tokens as usize, 1);
    for (i, &token) in tokens.iter().enumerate() {
        let is_last = i == tokens.len() - 1;
        batch
            .add(token, i as i32, &[0], is_last)
            .context("Failed to add token to batch")?;
    }
    ctx.decode(&mut batch)
        .context("Failed to run first forward pass")?;

    let mut sampler = LlamaSampler::greedy();
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut n_cur = n_prompt;
    let mut produced = String::new();
    const STOP_MARKERS: &[&str] = &["<|eot_id|>", "<|im_end|>"];

    loop {
        let next_token = sampler.sample(&ctx, -1);
        sampler.accept(next_token);

        if model.is_eog_token(next_token) || n_cur >= n_prompt + max_tokens as i32 {
            break;
        }

        let piece = model
            .token_to_piece(next_token, &mut decoder, false, None)
            .context("Failed to decode token to string")?;

        let prev_len = produced.len();
        produced.push_str(&piece);
        if let Some(idx) = STOP_MARKERS.iter().filter_map(|m| produced.find(m)).min() {
            if idx > prev_len {
                on_token(produced[prev_len..idx].to_string());
            }
            break;
        }

        on_token(piece);

        batch.clear();
        batch
            .add(next_token, n_cur, &[0], true)
            .context("Failed to add generated token to batch")?;
        n_cur += 1;

        ctx.decode(&mut batch)
            .context("Failed to run forward pass")?;
    }

    Ok(())
}

/// Convenience wrapper: loads a model from a file path, then generates.
/// Used in tests so they don't need to manage the backend/model themselves.
pub fn generate(model_path: &str, prompt: &str, max_tokens: u32) -> Result<String> {
    let backend = LlamaBackend::init().context("Failed to initialize llama.cpp backend")?;
    let model_params = LlamaModelParams::default();
    let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
        .context("Failed to load model file — check the path is correct")?;
    generate_with(&backend, &model, prompt, max_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Run with:
    //   $env:GREENCUBE_MODEL_PATH="C:\models\Qwen3-14B-Q4_K_M.gguf"
    //   cargo test test_basic_inference -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_basic_inference() {
        let model_path = std::env::var("GREENCUBE_MODEL_PATH")
            .expect("Set GREENCUBE_MODEL_PATH to the path of your .gguf file");

        let result = generate(&model_path, "The capital of France is", 20);

        assert!(result.is_ok(), "Inference failed: {:?}", result.err());
        let text = result.unwrap();
        println!("\n--- Model output ---\n{}\n--------------------", text);
        assert!(!text.is_empty(), "Expected non-empty output");
    }
}
