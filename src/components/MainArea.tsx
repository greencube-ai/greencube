import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const SUGGESTIONS = ["Organize my photos", "Write a story", "Help me code"];

const PHRASES = [
  "Ask GreenCube anything...",
  "Any shower thoughts?",
  "Have a startup idea?",
  "What should I cook tonight?",
  "Who is Scam Altman?",
  "Explain quantum physics to me like I'm 5",
  "Is Donald Trump an alien?",
  "Let's organise your photos",
  "Why is the sky blue?",
  "Plan my weekend",
];

interface Message {
  role: "user" | "assistant";
  content: string;
}

export default function MainArea() {
  const [inputValue, setInputValue] = useState("");
  const [phraseIndex, setPhraseIndex] = useState(0);
  const [phraseVisible, setPhraseVisible] = useState(false);
  const [messages, setMessages] = useState<Message[]>([]);
  const [streaming, setStreaming] = useState(false);
  const [streamingContent, setStreamingContent] = useState("");
  const messagesEndRef = useRef<HTMLDivElement>(null);

  // Rotating placeholder — only on home screen
  useEffect(() => {
    if (messages.length > 0) return;
    setPhraseVisible(true);
    const fadeOut = setTimeout(() => setPhraseVisible(false), 3300);
    const swap = setTimeout(() => setPhraseIndex((i) => (i + 1) % PHRASES.length), 3600);
    return () => { clearTimeout(fadeOut); clearTimeout(swap); };
  }, [phraseIndex, messages.length]);

  // Auto-scroll to latest message
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingContent]);

  async function sendMessage(text: string) {
    if (!text.trim() || streaming) return;

    const prompt = text.trim();
    setInputValue("");
    const history: Message[] = [...messages, { role: "user", content: prompt }];
    setMessages(history);
    setStreaming(true);
    setStreamingContent("");

    let response = "";

    const unlistenToken = await listen<string>("chat-token", (e) => {
      response += e.payload;
      setStreamingContent(response);
    });

    const unlistenDone = await listen("chat-done", () => {
      setMessages((prev) => [...prev, { role: "assistant", content: response }]);
      setStreamingContent("");
      setStreaming(false);
      unlistenToken();
      unlistenDone();
    });

    const unlistenError = await listen<string>("chat-error", (e) => {
      setMessages((prev) => [...prev, { role: "assistant", content: `Error: ${e.payload}` }]);
      setStreamingContent("");
      setStreaming(false);
      unlistenToken();
      unlistenDone();
      unlistenError();
    });

    try {
      await invoke("send_message_streaming", { messages: history });
    } catch (e) {
      setMessages((prev) => [...prev, { role: "assistant", content: `Error: ${e}` }]);
      setStreamingContent("");
      setStreaming(false);
      unlistenToken();
      unlistenDone();
      unlistenError();
    }
  }

  function handleKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendMessage(inputValue);
    }
  }

  // --- Chat view ---
  if (messages.length > 0) {
    return (
      <main className="flex-1 flex flex-col bg-cream overflow-hidden">
        <div className="flex-1 overflow-y-auto px-6 py-6">
          <div className="max-w-[700px] mx-auto flex flex-col gap-4">
            {messages.map((msg, i) => (
              <div key={i} className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}>
                <div
                  className={`max-w-[80%] px-4 py-3 rounded-xl text-[15px] whitespace-pre-wrap ${
                    msg.role === "user"
                      ? "bg-forest text-white"
                      : "bg-white text-ink border border-[#DDD8CE]"
                  }`}
                >
                  {msg.content}
                </div>
              </div>
            ))}

            {streaming && (
              <div className="flex justify-start">
                <div className="max-w-[80%] px-4 py-3 rounded-xl text-[15px] bg-white text-ink border border-[#DDD8CE] whitespace-pre-wrap">
                  {streamingContent || <span className="animate-pulse text-ink-soft">●●●</span>}
                </div>
              </div>
            )}

            <div ref={messagesEndRef} />
          </div>
        </div>

        <div className="px-6 py-4 border-t border-[#DDD8CE]">
          <div className="max-w-[700px] mx-auto flex gap-2">
            <input
              type="text"
              value={inputValue}
              onChange={(e) => setInputValue(e.target.value)}
              onKeyDown={handleKeyDown}
              disabled={streaming}
              placeholder="Message GreenCube..."
              className="flex-1 h-12 px-4 bg-white text-ink text-[15px] border-[1.5px] border-[#DDD8CE] rounded-lg outline-none disabled:opacity-50"
            />
            <button
              type="button"
              onClick={() => sendMessage(inputValue)}
              disabled={streaming || !inputValue.trim()}
              className="h-12 px-5 bg-forest text-white rounded-lg text-[14px] disabled:opacity-40 hover:opacity-90 transition-opacity cursor-pointer border-0"
            >
              Send
            </button>
          </div>
        </div>
      </main>
    );
  }

  // --- Home view ---
  return (
    <main className="flex-1 flex items-center justify-center bg-cream px-6">
      <div className="w-full max-w-[600px] flex flex-col items-center">
        <h1
          className="text-forest mb-6"
          style={{ fontFamily: "Georgia, serif", fontWeight: "bold", fontSize: "48px" }}
        >
          Create.
        </h1>

        <div className="relative w-full mb-4">
          <input
            type="text"
            value={inputValue}
            onChange={(e) => setInputValue(e.target.value)}
            onKeyDown={handleKeyDown}
            className="w-full h-12 px-4 bg-white text-ink text-[15px] border-[1.5px] border-[#DDD8CE] rounded-lg outline-none"
          />
          {inputValue === "" && (
            <div
              className="absolute inset-0 flex items-center pointer-events-none text-ink-soft text-[15px]"
              style={{ paddingLeft: "16px", opacity: phraseVisible ? 1 : 0, transition: "opacity 300ms ease-out" }}
            >
              {PHRASES[phraseIndex]}
            </div>
          )}
        </div>

        <div className="flex gap-3 mb-5 flex-wrap justify-center">
          {SUGGESTIONS.map((chip) => (
            <button
              key={chip}
              type="button"
              onClick={() => sendMessage(chip)}
              className="cursor-pointer bg-transparent border border-[#DDD8CE] text-ink-soft rounded-[20px] py-2 px-4 text-[13px] transition-colors duration-150 ease-out hover:border-moss hover:text-forest"
            >
              {chip}
            </button>
          ))}
        </div>

        <div className="text-ink-soft text-[12px]">
          Running locally · Private · No limits
        </div>
      </div>
    </main>
  );
}
