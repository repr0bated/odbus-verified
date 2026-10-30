import type { ExtensionAPI } from "@oh-my-pi/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  const baseUrl = process.env.FUSION_BRIDGE_URL;
  if (!baseUrl || !process.env.FUSION_BRIDGE_TOKEN) {
    throw new Error("Fusion agent must be started by run_fusion.py --agent");
  }
  pi.registerProvider("consolidation-fusion", {
    baseUrl,
    api: "openai-completions",
    apiKey: "FUSION_BRIDGE_TOKEN",
    models: [{
      id: "litellm/fusion-1",
      name: "LiteLLM Fusion — repository execution",
      reasoning: false,
      input: ["text"],
      contextWindow: 128000,
      maxTokens: 16000,
      compat: {
        supportsStore: false,
        supportsReasoningEffort: false,
        maxTokensField: "max_tokens",
      },
    }],
  });
}
