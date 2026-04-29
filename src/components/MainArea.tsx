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
  content: string; // raw, may contain <think>...</think>
}

interface StoredMessage {
  id: number;
  role: string;
  content: string;
}

interface Props {
  conversationId: string | null;
  onConversationCreated: (id: string) => void;
  onConversationUpdated: () => void;
}

// ---------------------------------------------------------------------------
// Think-block parsing
// ---------------------------------------------------------------------------

interface Parsed {
  thinking: string;
  response: string;
}

function parseThink(raw: string): Parsed {
  const start = raw.indexOf("<think>");
  if (start === -1) return { thinking: "", response: raw };

  const end = raw.indexOf("</think>");
  if (end === -1) {
    // Still inside the think block — everything after <think> is thinking.
    return { thinking: raw.slice(start + 7), response: "" };
  }

  const thinking = raw.slice(start + 7, end);
  const response = raw.slice(end + 8).replace(/^\n+/, "");
  return { thinking, response };
}

// ---------------------------------------------------------------------------
// ThinkSection — collapsible think block shown above assistant messages
// ---------------------------------------------------------------------------

function ThinkSection({
  thinking,
  defaultOpen = false,
}: {
  thinking: string;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <div className="mb-2">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex items-center gap-1 text-[12px] text-ink-soft hover:text-ink cursor-pointer bg-transparent border-0 p-0 transition-colors"
      >
        <span className="text-[10px]">{open ? "▾" : "▸"}</span>
        <span>Thought for a moment</span>
      </button>
      {open && (
        <div className="mt-1 pl-3 border-l-2 border-[#DDD8CE] text-[13px] text-ink-soft italic whitespace-pre-wrap leading-relaxed">
          {thinking}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// AssistantBubble — completed assistant message with optional think section
// ---------------------------------------------------------------------------

function AssistantBubble({ content }: { content: string }) {
  const { thinking, response } = parseThink(content);
  const displayText = response || content; // fallback if no closing tag

  return (
    <div className="max-w-[80%] px-4 py-3 rounded-xl text-[15px] bg-white text-ink border border-[#DDD8CE]">
      {thinking && <ThinkSection thinking={thinking} defaultOpen={false} />}
      <div className="whitespace-pre-wrap">{displayText}</div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// StreamingBubble — live bubble shown while the model is generating
// ---------------------------------------------------------------------------

function StreamingBubble({ raw }: { raw: string }) {
  const { thinking, response } = parseThink(raw);

  if (!raw) {
    return (
      <div className="max-w-[80%] px-4 py-3 rounded-xl text-[15px] bg-white text-ink border border-[#DDD8CE]">
        <span className="animate-pulse text-ink-soft">●●●</span>
      </div>
    );
  }

  return (
    <div className="max-w-[80%] px-4 py-3 rounded-xl text-[15px] bg-white text-ink border border-[#DDD8CE]">
      {thinking && (
        <div className="mb-2">
          <div className="flex items-center gap-1 text-[12px] text-ink-soft mb-1">
            <span className="animate-pulse">◌</span>
            <span>Thinking...</span>
          </div>
          <div className="pl-3 border-l-2 border-[#DDD8CE] text-[13px] text-ink-soft italic whitespace-pre-wrap leading-relaxed">
            {thinking}
          </div>
        </div>
      )}
      {response && (
        <div className="whitespace-pre-wrap">{response}</div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// MainArea
// ---------------------------------------------------------------------------

export default function MainArea({
  conversationId,
  onConversationCreated,
  onConversationUpdated,
}: Props) {
  const [inputValue, setInputValue] = useState("");
  const [phraseIndex, setPhraseIndex] = useState(0);
  const [phraseVisible, setPhraseVisible] = useState(false);
  const [messages, setMessages] = useState<Message[]>([]);
  const [streaming, setStreaming] = useState(false);
  const [streamingContent, setStreamingContent] = useState("");

  // Source of truth for the active conversation — may change from null to a
  // UUID mid-session without causing a remount.
  const activeConvId = useRef<string | null>(conversationId);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  // Load messages when mounted on an existing conversation.
  useEffect(() => {
    if (!conversationId) return;
    invoke<StoredMessage[]>("load_conversation", { id: conversationId })
      .then((msgs) =>
        setMessages(
          msgs.map((m) => ({
            role: m.role as "user" | "assistant",
            content: m.content,
          }))
        )
      )
      .catch((e) => console.error("load_conversation failed:", e));
  }, []);

  // Rotating placeholder — only on home screen.
  useEffect(() => {
    if (messages.length > 0) return;
    setPhraseVisible(true);
    const fadeOut = setTimeout(() => setPhraseVisible(false), 3300);
    const swap = setTimeout(
      () => setPhraseIndex((i) => (i + 1) % PHRASES.length),
      3600
    );
    return () => {
      clearTimeout(fadeOut);
      clearTimeout(swap);
    };
  }, [phraseIndex, messages.length]);

  // Auto-scroll.
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingContent]);

  async function sendMessage(text: string) {
    if (!text.trim() || streaming) return;

    const prompt = text.trim();
    setInputValue("");
    setMessages((prev) => [...prev, { role: "user", content: prompt }]);
    setStreaming(true);
    setStreamingContent("");

    let response = "";

    const unlistenToken = await listen<string>("chat-token", (e) => {
      response += e.payload;
      setStreamingContent(response);
    });

    const unlistenDone = await listen("chat-done", () => {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: response },
      ]);
      setStreamingContent("");
      setStreaming(false);
      onConversationUpdated();
      unlistenToken();
      unlistenDone();
    });

    const unlistenError = await listen<string>("chat-error", (e) => {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: `Error: ${e.payload}` },
      ]);
      setStreamingContent("");
      setStreaming(false);
      unlistenToken();
      unlistenDone();
      unlistenError();
    });

    try {
      const returnedConvId = await invoke<string>("send_message_streaming", {
        prompt,
        conversationId: activeConvId.current,
      });

      // New conversation was just created — notify App (sidebar refresh).
      // This does NOT remount MainArea; streaming continues uninterrupted.
      if (!activeConvId.current) {
        activeConvId.current = returnedConvId;
        onConversationCreated(returnedConvId);
      }
    } catch (e) {
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: `Error: ${e}` },
      ]);
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
  if (messages.length > 0 || streaming) {
    return (
      <main className="flex-1 flex flex-col bg-cream overflow-hidden">
        <div className="flex-1 overflow-y-auto px-6 py-6">
          <div className="max-w-[700px] mx-auto flex flex-col gap-4">
            {messages.map((msg, i) => (
              <div
                key={i}
                className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}
              >
                {msg.role === "user" ? (
                  <div className="max-w-[80%] px-4 py-3 rounded-xl text-[15px] bg-forest text-white whitespace-pre-wrap">
                    {msg.content}
                  </div>
                ) : (
                  <AssistantBubble content={msg.content} />
                )}
              </div>
            ))}

            {streaming && (
              <div className="flex justify-start">
                <StreamingBubble raw={streamingContent} />
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
              style={{
                paddingLeft: "16px",
                opacity: phraseVisible ? 1 : 0,
                transition: "opacity 300ms ease-out",
              }}
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
