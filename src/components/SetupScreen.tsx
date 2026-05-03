import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface ModelStatus {
  id: string;
  name: string;
  filename: string;
  size_bytes: number;
  min_ram_gb: number;
  recommended: boolean;
  downloaded: boolean;
  path: string;
}

interface HardwareProfile {
  total_ram_gb: number;
  cpu_threads: number;
  has_battery: boolean;
  is_laptop_likely: boolean;
  on_battery_power: boolean;
  recommended_model_id: string;
  recommended_model_name: string;
  recommendation_reason: string;
}

interface DownloadProgress {
  model_id: string;
  downloaded_bytes: number;
  total_bytes: number;
}

interface DownloadComplete {
  model_id: string;
  path: string;
}

interface DownloadError {
  model_id: string;
  error: string;
}

interface Props {
  onClose: () => void;
}


export default function SetupScreen({ onClose }: Props) {
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [hardware, setHardware] = useState<HardwareProfile | null>(null);
  const [progress, setProgress] = useState<Record<string, number>>({}); // model_id → 0-100
  const [downloading, setDownloading] = useState<Set<string>>(new Set());
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [devPin, setDevPin] = useState<string | null>(null); // currently pinned model id
  const [tab, setTab] = useState<"models" | "developer">("models");
  const [hfToken, setHfToken] = useState("");

  useEffect(() => {
    invoke<ModelStatus[]>("list_models")
      .then(setModels)
      .catch((e) => console.error("list_models failed:", e));

    invoke<HardwareProfile>("get_hardware_profile")
      .then(setHardware)
      .catch((e) => console.error("get_hardware_profile failed:", e));

    invoke<string | null>("get_dev_model")
      .then(setDevPin)
      .catch(() => {});

    const unlistenProgress = listen<DownloadProgress>("download-progress", (e) => {
      const { model_id, downloaded_bytes, total_bytes } = e.payload;
      const pct = total_bytes > 0 ? Math.round((downloaded_bytes / total_bytes) * 100) : 0;
      setProgress((prev) => ({ ...prev, [model_id]: pct }));
    });

    const unlistenComplete = listen<DownloadComplete>("download-complete", (e) => {
      const { model_id } = e.payload;
      setDownloading((prev) => {
        const next = new Set(prev);
        next.delete(model_id);
        return next;
      });
      setProgress((prev) => ({ ...prev, [model_id]: 100 }));
      // Refresh model list so downloaded flag updates.
      invoke<ModelStatus[]>("list_models").then(setModels).catch(() => {});
    });

    const unlistenError = listen<DownloadError>("download-error", (e) => {
      const { model_id, error } = e.payload;
      setDownloading((prev) => {
        const next = new Set(prev);
        next.delete(model_id);
        return next;
      });
      setErrors((prev) => ({ ...prev, [model_id]: error }));
    });

    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenComplete.then((fn) => fn());
      unlistenError.then((fn) => fn());
    };
  }, []);

  function startDownload(modelId: string) {
    setErrors((prev) => {
      const next = { ...prev };
      delete next[modelId];
      return next;
    });
    setDownloading((prev) => new Set(prev).add(modelId));
    setProgress((prev) => ({ ...prev, [modelId]: 0 }));
    invoke("download_model", { modelId, hfToken: hfToken.trim() || null }).catch((e) => {
      setErrors((prev) => ({ ...prev, [modelId]: String(e) }));
      setDownloading((prev) => {
        const next = new Set(prev);
        next.delete(modelId);
        return next;
      });
    });
  }

  function pinModel(modelId: string | null) {
    invoke("set_dev_model", { modelId })
      .then(() => setDevPin(modelId))
      .catch((e) => console.error("set_dev_model failed:", e));
  }

  const anyDownloaded = models.some((m) => m.downloaded);

  return (
    <div className="fixed inset-0 z-40 bg-cream flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between px-8 py-5 border-b border-[#DDD8CE]">
        <div className="flex items-center gap-6">
          <span
            className="text-forest"
            style={{ fontFamily: "Georgia, serif", fontWeight: "bold", fontSize: "20px" }}
          >
            GreenCube · Settings
          </span>
          <div className="flex gap-1">
            {(["models", "developer"] as const).map((t) => (
              <button
                key={t}
                type="button"
                onClick={() => setTab(t)}
                className={`text-[12px] px-3 py-1 rounded-full border-0 cursor-pointer capitalize transition-colors ${
                  tab === t
                    ? "bg-forest text-white"
                    : "bg-transparent text-ink-soft hover:text-ink"
                }`}
              >
                {t}
              </button>
            ))}
          </div>
        </div>
        {anyDownloaded && (
          <button
            type="button"
            onClick={onClose}
            className="text-[13px] px-4 py-2 bg-forest text-white rounded-lg border-0 cursor-pointer hover:opacity-90"
          >
            Done
          </button>
        )}
      </div>

      {/* Body */}
      <div className="flex-1 overflow-y-auto px-8 py-6">
        {tab === "models" && (
          <>
            <p className="text-ink-soft text-[13px] mb-4 max-w-[600px]">
              GreenCube runs AI entirely on your device. Download a model to get started.
              The recommended model is picked from your RAM, CPU profile, and portable power state.
            </p>

            {hardware && (
              <div className="mb-5 max-w-[640px] rounded-xl border border-[#DDD8CE] bg-white px-4 py-3">
                <div className="text-[13px] text-ink">
                  Detected {hardware.total_ram_gb} GB RAM · {hardware.cpu_threads} CPU threads ·{" "}
                  {hardware.is_laptop_likely
                    ? hardware.on_battery_power
                      ? "portable device on battery"
                      : "portable device on AC power"
                    : "desktop-style power profile"}
                </div>
                <div className="text-[12px] text-ink-soft mt-1 leading-snug">
                  Recommended: {hardware.recommended_model_name}. {hardware.recommendation_reason}
                </div>
              </div>
            )}

            <div className="mb-6 max-w-[640px]">
              <label className="block text-[12px] text-ink-soft mb-1">
                HuggingFace token{" "}
                <span className="opacity-60">(required for Llama — accept the license at huggingface.co first)</span>
              </label>
              <input
                type="password"
                value={hfToken}
                onChange={(e) => setHfToken(e.target.value)}
                placeholder="hf_..."
                className="w-full h-9 px-3 bg-white text-ink text-[13px] border border-[#DDD8CE] rounded-lg outline-none focus:border-forest"
              />
            </div>

            <div className="flex flex-col gap-4 max-w-[640px]">
              {models.map((m) => {
                const isDownloading = downloading.has(m.id);
                const pct = progress[m.id] ?? 0;
                const err = errors[m.id];

                return (
                  <div
                    key={m.id}
                    className={`rounded-xl border px-5 py-4 bg-white ${
                      m.recommended ? "border-forest" : "border-[#DDD8CE]"
                    }`}
                  >
                    <div className="flex items-start justify-between gap-4">
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 flex-wrap">
                          <span className="text-[14px] font-medium text-ink">{m.name}</span>
                          {m.recommended && (
                            <span className="text-[11px] px-2 py-0.5 bg-forest text-white rounded-full">
                              Recommended
                            </span>
                          )}
                          {m.downloaded && (
                            <span className="text-[11px] px-2 py-0.5 bg-[#E2DED5] text-ink-soft rounded-full">
                              ✓ Downloaded
                            </span>
                          )}
                        </div>
                        <div className="text-[12px] text-ink-soft mt-0.5">
                          Requires {m.min_ram_gb > 0 ? `${m.min_ram_gb} GB` : "any"} RAM
                        </div>
                      </div>

                      {!m.downloaded && !isDownloading && (
                        <button
                          type="button"
                          onClick={() => startDownload(m.id)}
                          className="shrink-0 text-[13px] px-4 py-1.5 bg-forest text-white rounded-lg border-0 cursor-pointer hover:opacity-90"
                        >
                          Download
                        </button>
                      )}
                    </div>

                    {/* Progress bar */}
                    {isDownloading && (
                      <div className="mt-3">
                        <div className="flex justify-between text-[11px] text-ink-soft mb-1">
                          <span>Downloading…</span>
                          <span>{pct}%</span>
                        </div>
                        <div className="w-full h-1.5 bg-[#E8E4DB] rounded-full overflow-hidden">
                          <div
                            className="h-full bg-forest rounded-full transition-all duration-300"
                            style={{ width: `${pct}%` }}
                          />
                        </div>
                      </div>
                    )}

                    {/* Error */}
                    {err && (
                      <div className="mt-2 text-[12px] text-red-600 leading-snug">⚠ {err}</div>
                    )}
                  </div>
                );
              })}
            </div>
          </>
        )}

        {tab === "developer" && (
          <>
            <p className="text-ink-soft text-[13px] mb-2 max-w-[600px]">
              Pin a specific model for all responses — useful for testing smaller models on powerful hardware.
              Auto-selection (fast/reasoning switching) resumes when you clear the pin.
            </p>
            {devPin && (
              <div className="mb-4 text-[12px] text-amber-700 bg-amber-50 border border-amber-200 rounded-lg px-4 py-2 max-w-[640px]">
                Auto-selection is <strong>disabled</strong>. All responses use the pinned model.
              </div>
            )}

            <div className="flex flex-col gap-3 max-w-[640px]">
              {/* "Use auto" option */}
              <div
                className={`rounded-xl border px-5 py-4 bg-white flex items-center justify-between gap-4 ${
                  devPin === null ? "border-forest" : "border-[#DDD8CE]"
                }`}
              >
                <div>
                  <div className="text-[14px] font-medium text-ink">Auto-selection</div>
                  <div className="text-[12px] text-ink-soft mt-0.5">
                    Automatically picks Fast or Reasoning based on prompt complexity
                  </div>
                </div>
                {devPin === null ? (
                  <span className="shrink-0 text-[11px] px-3 py-1.5 bg-forest text-white rounded-lg">
                    Active
                  </span>
                ) : (
                  <button
                    type="button"
                    onClick={() => pinModel(null)}
                    className="shrink-0 text-[13px] px-3 py-1.5 bg-transparent border border-[#DDD8CE] text-ink-soft rounded-lg cursor-pointer hover:border-forest hover:text-forest transition-colors"
                  >
                    Use auto
                  </button>
                )}
              </div>

              {/* One row per model */}
              {models.map((m) => {
                const isPinned = devPin === m.id;
                return (
                  <div
                    key={m.id}
                    className={`rounded-xl border px-5 py-4 bg-white flex items-center justify-between gap-4 ${
                      isPinned ? "border-forest" : "border-[#DDD8CE]"
                    } ${!m.downloaded ? "opacity-50" : ""}`}
                  >
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="text-[14px] font-medium text-ink">{m.name}</span>
                        {isPinned && (
                          <span className="text-[11px] px-2 py-0.5 bg-forest text-white rounded-full">
                            Pinned
                          </span>
                        )}
                        {!m.downloaded && (
                          <span className="text-[11px] text-ink-soft">Not downloaded</span>
                        )}
                      </div>
                      <div className="text-[12px] text-ink-soft mt-0.5">
                        Requires {m.min_ram_gb > 0 ? `${m.min_ram_gb} GB` : "any"} RAM
                      </div>
                    </div>
                    {m.downloaded && !isPinned && (
                      <button
                        type="button"
                        onClick={() => pinModel(m.id)}
                        className="shrink-0 text-[13px] px-3 py-1.5 bg-transparent border border-[#DDD8CE] text-ink-soft rounded-lg cursor-pointer hover:border-forest hover:text-forest transition-colors"
                      >
                        Use for testing
                      </button>
                    )}
                    {isPinned && (
                      <button
                        type="button"
                        onClick={() => pinModel(null)}
                        className="shrink-0 text-[13px] px-3 py-1.5 bg-transparent border border-[#DDD8CE] text-ink-soft rounded-lg cursor-pointer hover:border-red-400 hover:text-red-500 transition-colors"
                      >
                        Unpin
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
