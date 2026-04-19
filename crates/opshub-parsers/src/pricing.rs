//! USD-per-million-token pricing for models we know about. Prices are
//! hard-coded — a pragmatic trade-off for an offline MVP — and must be
//! refreshed when providers change their rate card.
//!
//! Sources (last audited 2026-04-19):
//! - Anthropic: <https://www.anthropic.com/pricing>
//! - OpenAI:    <https://openai.com/api/pricing/>
//! - Moonshot (Kimi) and Zhipu (GLM) pricing is approximate; unknown models
//!   silently produce a $0 estimate.

#[derive(Debug, Clone, Copy)]
pub struct ModelPrice {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_write_per_mtok: f64,
}

/// Best-effort price lookup. Returns `None` for unknown models so callers can
/// degrade gracefully (we log a warning once instead of panicking).
pub fn lookup(model: &str) -> Option<ModelPrice> {
    let m = model.to_ascii_lowercase();
    let mm = m.as_str();
    Some(match mm {
        _ if mm.starts_with("claude-opus-4") || mm.starts_with("claude-opus-5") || mm == "opus" => {
            ModelPrice {
                input_per_mtok: 15.0,
                output_per_mtok: 75.0,
                cache_read_per_mtok: 1.5,
                cache_write_per_mtok: 18.75,
            }
        }
        _ if mm.starts_with("claude-sonnet-4") || mm == "sonnet" => ModelPrice {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.3,
            cache_write_per_mtok: 3.75,
        },
        _ if mm.starts_with("claude-haiku-4") || mm == "haiku" => ModelPrice {
            input_per_mtok: 1.0,
            output_per_mtok: 5.0,
            cache_read_per_mtok: 0.1,
            cache_write_per_mtok: 1.25,
        },
        _ if mm.starts_with("gpt-5") => ModelPrice {
            input_per_mtok: 2.5,
            output_per_mtok: 10.0,
            cache_read_per_mtok: 0.25,
            cache_write_per_mtok: 2.5,
        },
        _ if mm.starts_with("kimi") => ModelPrice {
            input_per_mtok: 0.6,
            output_per_mtok: 2.5,
            cache_read_per_mtok: 0.06,
            cache_write_per_mtok: 0.6,
        },
        _ if mm.starts_with("glm") => ModelPrice {
            input_per_mtok: 0.5,
            output_per_mtok: 1.5,
            cache_read_per_mtok: 0.05,
            cache_write_per_mtok: 0.5,
        },
        _ => return None,
    })
}

pub fn estimate(
    model: &str,
    input_tok: i64,
    output_tok: i64,
    cache_read: i64,
    cache_write: i64,
) -> f64 {
    let Some(p) = lookup(model) else {
        return 0.0;
    };
    let mm = 1_000_000.0;
    (input_tok as f64) * p.input_per_mtok / mm
        + (output_tok as f64) * p.output_per_mtok / mm
        + (cache_read as f64) * p.cache_read_per_mtok / mm
        + (cache_write as f64) * p.cache_write_per_mtok / mm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_models_are_priced() {
        assert!(lookup("claude-sonnet-4-6").is_some());
        assert!(lookup("claude-opus-4-7").is_some());
        assert!(lookup("claude-haiku-4-5-20251001").is_some());
        assert!(lookup("gpt-5.4").is_some());
        assert!(lookup("kimi-k2.5").is_some());
        assert!(lookup("glm-5").is_some());
    }

    #[test]
    fn unknown_model_yields_zero() {
        assert_eq!(estimate("fictional-model-9000", 1_000, 1_000, 0, 0), 0.0);
    }

    #[test]
    fn sonnet_math_is_sane() {
        // 1M input + 1M output @ $3 + $15
        let usd = estimate("claude-sonnet-4-6", 1_000_000, 1_000_000, 0, 0);
        assert!((usd - 18.0).abs() < 1e-9, "expected $18, got {usd}");
    }
}
