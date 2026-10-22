use serde_json::{json, Value};

pub(crate) fn provider(port: u16, key: &str, context: u32, quantization: &str) -> Value {
    json!({
        "model": "strata/local",
        "agents": {"title": {"model": "strata/local"}},
        "policies": [
            {"action": "provider.use", "resource": "*", "effect": "deny"},
            {"action": "provider.use", "resource": "strata", "effect": "allow"}
        ],
        "update": "disable", "share": "disabled", "plugins": [],
        "mcp": {}, "lsp": false, "formatter": false,
        // Leave 16K output plus 4K for new tool results before compacting.
        "compaction": {"auto": true, "buffer": 20480, "keep": {"tokens": 4096}},
        "tool_output": {"max_lines": 400, "max_bytes": 16000},
        "providers": {"strata": {
            "package": "@ai-sdk/openai-compatible", "name": "Strata (local)",
            "settings": {"baseURL": format!("http://127.0.0.1:{port}/v1"), "apiKey": key},
            "models": {"local": {
                "name": format!("Qwen3.8 Flash Next {quantization}"),
                "capabilities": {"tools": true, "input": ["text"], "output": ["text"]},
                "limit": {"context": context, "input": context, "output": 16384}
            }}
        }}
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_uses_local_model_for_all_requests() {
        let config = provider(12345, "test-key", 65536, "IQ3_XXS");
        assert_eq!(config["model"], config["agents"]["title"]["model"]);
        let output = config["providers"]["strata"]["models"]["local"]["limit"]["output"]
            .as_u64()
            .unwrap();
        let reserved = config["compaction"]["buffer"].as_u64().unwrap();
        assert!(reserved >= output + 4096);
        assert!(reserved < 32768);
        assert_eq!(
            config["providers"]["strata"]["models"]["local"]["name"],
            "Qwen3.8 Flash Next IQ3_XXS"
        );
        assert_eq!(
            config["providers"]["strata"]["settings"]["baseURL"],
            "http://127.0.0.1:12345/v1"
        );
    }
}
