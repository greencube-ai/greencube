import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ConversationSummary {
  id: string;
  title: string;
  updated_at: number;
}

interface Memory {
  id: number;
  content: string;
  created_at: number;
}

interface FileMemoryContent {
  filename: string;
  content: string;
  truncated: boolean;
}

interface Props {
  onNewChat?: () => void;
  activeConversationId: string | null;
  onSelectConversation: (id: string) => void;
  refreshKey: number;
  onOpenSetup?: () => void;
}

export default function Sidebar({
  onNewChat,
  activeConversationId,
  onSelectConversation,
  refreshKey,
  onOpenSetup,
}: Props) {
  const [collapsed, setCollapsed] = useState(false);
  const [activeTab, setActiveTab] = useState<"chats" | "memory">("chats");

  // Chats
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);
  const [deletingConversationId, setDeletingConversationId] = useState<string | null>(null);

  // Memory
  const [memories, setMemories] = useState<Memory[]>([]);
  const [addingMemory, setAddingMemory] = useState(false);
  const [newMemoryText, setNewMemoryText] = useState("");
  const [fileLoading, setFileLoading] = useState(false);
  const [fileError, setFileError] = useState<string | null>(null);
  const [savedToast, setSavedToast] = useState(false);

  // Drag-and-drop
  const [isDragOver, setIsDragOver] = useState(false);
  // Counter approach prevents spurious dragLeave events when cursor moves
  // between child elements inside the drop zone.
  const dragCounter = useRef(0);

  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    invoke<ConversationSummary[]>("list_conversations")
      .then(setConversations)
      .catch((e) => console.error("list_conversations failed:", e));
  }, [refreshKey]);

  useEffect(() => {
    fetchMemories();
  }, []);

  function fetchMemories() {
    invoke<Memory[]>("list_memories")
      .then(setMemories)
      .catch((e) => console.error("list_memories failed:", e));
  }

  function showSavedFeedback() {
    setSavedToast(true);
    setTimeout(() => setSavedToast(false), 2000);
  }

  async function deleteConversation(
    e: React.MouseEvent<HTMLButtonElement>,
    conversationId: string,
    title: string
  ) {
    e.stopPropagation();

    const confirmed = window.confirm(`Delete "${title}"?`);
    if (!confirmed) return;

    setDeletingConversationId(conversationId);
    try {
      await invoke("delete_conversation", { id: conversationId });
      setConversations((prev) => prev.filter((conv) => conv.id !== conversationId));

      if (activeConversationId === conversationId) {
        onNewChat?.();
      }
    } catch (err) {
      console.error("delete_conversation failed:", err);
    } finally {
      setDeletingConversationId((current) =>
        current === conversationId ? null : current
      );
    }
  }

  // ── File reading ──────────────────────────────────────────────────────────

  const MAX_TEXT_CHARS = 8_000;

  function truncate(text: string): string {
    const count = text.length;
    if (count <= MAX_TEXT_CHARS) return text;
    return (
      text.slice(0, MAX_TEXT_CHARS) +
      `\n\n[… file truncated — showing ${MAX_TEXT_CHARS} of ${count} characters]`
    );
  }

  async function extractContent(file: File): Promise<string> {
    const isPdf = file.name.toLowerCase().endsWith(".pdf");

    // Primary: Tauri injects a non-standard .path property on File objects.
    // When available this is faster (Rust reads directly from disk).
    const filePath: string | undefined = (file as any).path;
    if (filePath) {
      try {
        const result = await invoke<FileMemoryContent>("read_file_for_memory", {
          path: filePath,
        });
        return result.content;
      } catch {
        // fall through to browser-based fallback
      }
    }

    // Fallback A — PDF: read bytes in the browser, send to Rust for extraction.
    if (isPdf) {
      const buffer = await file.arrayBuffer();
      // Convert to a plain number array for Tauri's JSON IPC.
      const bytes = Array.from(new Uint8Array(buffer));
      return await invoke<string>("extract_pdf_bytes", { bytes });
    }

    // Fallback B — text files: read directly in the browser.
    const text = await file.text();
    return truncate(text);
  }

  async function processFile(file: File) {
    setFileLoading(true);
    setFileError(null);
    try {
      const content = await extractContent(file);
      setNewMemoryText(`File: ${file.name}\n\n${content}`);
      setAddingMemory(true);
      setTimeout(() => textareaRef.current?.focus(), 50);
    } catch (err) {
      setFileError(`Could not read file: ${err}`);
    } finally {
      setFileLoading(false);
    }
  }

  async function handleFileInputChange(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    await processFile(file);
    if (fileInputRef.current) fileInputRef.current.value = "";
  }

  // ── Drag-and-drop handlers ────────────────────────────────────────────────

  function handleDragEnter(e: React.DragEvent) {
    if (!e.dataTransfer.types.includes("Files")) return;
    e.preventDefault();
    dragCounter.current += 1;
    if (dragCounter.current === 1) {
      // First enter — switch to Memory tab and show the drop overlay.
      setActiveTab("memory");
      setIsDragOver(true);
    }
  }

  function handleDragOver(e: React.DragEvent) {
    if (!e.dataTransfer.types.includes("Files")) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy"; // eslint-disable-line no-param-reassign
  }

  function handleDragLeave(_e: React.DragEvent) {
    dragCounter.current -= 1;
    if (dragCounter.current === 0) {
      setIsDragOver(false);
    }
  }

  async function handleDrop(e: React.DragEvent) {
    e.preventDefault();
    dragCounter.current = 0;
    setIsDragOver(false);

    const file = e.dataTransfer.files[0];
    if (!file) return;
    await processFile(file);
  }

  // ── Memory CRUD ───────────────────────────────────────────────────────────

  function openTextMemory() {
    setNewMemoryText("");
    setAddingMemory(true);
    setTimeout(() => textareaRef.current?.focus(), 50);
  }

  async function saveMemory() {
    const text = newMemoryText.trim();
    if (!text) return;
    try {
      await invoke("add_memory", { content: text });
      setAddingMemory(false);
      setNewMemoryText("");
      fetchMemories();
      showSavedFeedback();
    } catch (e) {
      console.error("add_memory failed:", e);
    }
  }

  async function deleteMemory(id: number) {
    try {
      await invoke("delete_memory", { id });
      setMemories((prev) => prev.filter((m) => m.id !== id));
    } catch (e) {
      console.error("delete_memory failed:", e);
    }
  }

  function handleMemoryKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      saveMemory();
    }
    if (e.key === "Escape") {
      setAddingMemory(false);
    }
  }

  return (
    <aside
      className="flex flex-col h-full bg-[#F0ECE3] border-r border-[#DDD8CE] shrink-0 transition-all duration-150 ease-out relative"
      style={{ width: collapsed ? "56px" : "260px" }}
      onDragEnter={handleDragEnter}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {/* ── Drag-over overlay (covers the whole sidebar) ── */}
      {isDragOver && !collapsed && (
        <div className="absolute inset-0 z-20 flex flex-col items-center justify-center bg-[#F0ECE3]/90 border-2 border-dashed border-forest rounded pointer-events-none">
          <span className="text-3xl mb-2">📎</span>
          <span className="text-forest text-[13px] font-medium">Drop to add to memory</span>
        </div>
      )}

      {/* ── Header ── */}
      <div className="flex items-center justify-between p-4">
        {!collapsed && (
          <span
            className="text-forest"
            style={{ fontFamily: "Georgia, serif", fontWeight: "bold", fontSize: "16px" }}
          >
            GreenCube
          </span>
        )}
        <button
          type="button"
          onClick={() => setCollapsed(!collapsed)}
          aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          className="text-sage hover:text-cream cursor-pointer bg-transparent border-0 text-[18px] leading-none transition-colors duration-150 ease-out"
        >
          {collapsed ? "›" : "‹"}
        </button>
      </div>

      {!collapsed && (
        <>
          {/* ── Tabs ── */}
          <div className="flex px-4 gap-1 pb-3">
            <button
              type="button"
              onClick={() => setActiveTab("chats")}
              className={`text-[12px] px-3 py-1 rounded-full border-0 cursor-pointer transition-colors duration-150 ${
                activeTab === "chats"
                  ? "bg-forest text-white"
                  : "bg-transparent text-ink-soft hover:text-ink"
              }`}
            >
              Chats
            </button>
            <button
              type="button"
              onClick={() => setActiveTab("memory")}
              className={`text-[12px] px-3 py-1 rounded-full border-0 cursor-pointer transition-colors duration-150 ${
                activeTab === "memory"
                  ? "bg-forest text-white"
                  : "bg-transparent text-ink-soft hover:text-ink"
              }`}
            >
              Memory
            </button>
          </div>

          {/* ── Chats tab ── */}
          {activeTab === "chats" && (
            <>
              <div className="px-4 pb-1">
                <button
                  type="button"
                  onClick={onNewChat}
                  className="bg-transparent text-ink-soft hover:text-ink border-0 p-0 text-[13px] text-left cursor-pointer transition-colors duration-150 ease-out"
                >
                  + New chat
                </button>
              </div>

              <div className="text-ink-soft uppercase tracking-[2px] text-[10px] pt-4 px-4 pb-2">
                RECENT
              </div>

              <nav className="flex-1 overflow-y-auto px-2">
                {conversations.length === 0 ? (
                  <div className="text-ink-soft text-[12px] px-3 py-2">
                    No conversations yet
                  </div>
                ) : (
                  conversations.map((conv) => (
                    <div
                      key={conv.id}
                      onClick={() => onSelectConversation(conv.id)}
                      className={`group flex items-center gap-2 text-ink text-[13px] py-2 px-3 rounded-sm cursor-pointer transition-colors duration-150 ease-out ${
                        activeConversationId === conv.id
                          ? "bg-[#E2DED5]"
                          : "hover:bg-[#E8E4DB]"
                      }`}
                      title={conv.title}
                    >
                      <span className="flex-1 truncate">{conv.title}</span>
                      <button
                        type="button"
                        onClick={(e) => deleteConversation(e, conv.id, conv.title)}
                        aria-label={`Delete conversation ${conv.title}`}
                        disabled={deletingConversationId === conv.id}
                        className="text-ink-soft hover:text-red-400 cursor-pointer bg-transparent border-0 text-[16px] leading-none opacity-0 group-hover:opacity-100 transition-opacity shrink-0 disabled:opacity-40"
                      >
                        {deletingConversationId === conv.id ? "..." : "×"}
                      </button>
                    </div>
                  ))
                )}
              </nav>

              <button
                type="button"
                onClick={onOpenSetup}
                className="w-full text-left text-ink hover:text-forest cursor-pointer text-[14px] px-4 pt-4 pb-[20px] transition-colors duration-150 ease-out bg-transparent border-0"
              >
                <span className="mr-2">⚙️</span>
                Models
              </button>
            </>
          )}

          {/* ── Memory tab ── */}
          {activeTab === "memory" && (
            <div className="flex-1 overflow-y-auto flex flex-col px-3 gap-2 pb-4">
              <div className="text-ink-soft uppercase tracking-[2px] text-[10px] pt-1 px-1 pb-1">
                WHAT I KNOW
              </div>

              {/* Saved toast */}
              {savedToast && (
                <div className="flex items-center gap-2 px-3 py-2 bg-forest text-white text-[12px] rounded-lg">
                  <span>✓</span>
                  <span>Memory saved</span>
                </div>
              )}

              {/* File error */}
              {fileError && (
                <div className="flex items-start gap-2 px-3 py-2 bg-red-50 border border-red-200 text-red-700 text-[12px] rounded-lg">
                  <span className="shrink-0 mt-[1px]">⚠</span>
                  <span className="leading-snug">{fileError}</span>
                </div>
              )}

              {memories.length === 0 && !addingMemory && (
                <p className="text-ink-soft text-[12px] px-1 leading-snug">
                  Nothing saved yet. Add notes or drop a file here for the AI to
                  always remember.
                </p>
              )}

              {/* Memory cards */}
              {memories.map((mem) => (
                <div
                  key={mem.id}
                  className="group flex items-start gap-2 bg-white rounded-lg px-3 py-2 text-[13px] text-ink border border-[#DDD8CE]"
                >
                  <span className="flex-1 leading-snug break-words min-w-0">
                    {mem.content.startsWith("File: ")
                      ? mem.content.split("\n")[0]
                      : mem.content}
                  </span>
                  <button
                    type="button"
                    onClick={() => deleteMemory(mem.id)}
                    aria-label="Delete memory"
                    className="text-ink-soft hover:text-red-400 cursor-pointer bg-transparent border-0 text-[16px] leading-none opacity-0 group-hover:opacity-100 transition-opacity shrink-0 mt-[1px]"
                  >
                    ×
                  </button>
                </div>
              ))}

              {/* Inline editor */}
              {addingMemory && (
                <div className="flex flex-col gap-1">
                  <textarea
                    ref={textareaRef}
                    value={newMemoryText}
                    onChange={(e) => setNewMemoryText(e.target.value)}
                    onKeyDown={handleMemoryKeyDown}
                    rows={5}
                    placeholder="Something to remember…"
                    className="w-full text-[13px] text-ink border border-[#DDD8CE] rounded-lg p-2 outline-none resize-none bg-white"
                  />
                  <div className="flex gap-2">
                    <button
                      type="button"
                      onClick={saveMemory}
                      className="text-[12px] px-3 py-1 bg-forest text-white rounded-md border-0 cursor-pointer hover:opacity-90"
                    >
                      Save
                    </button>
                    <button
                      type="button"
                      onClick={() => setAddingMemory(false)}
                      className="text-[12px] px-3 py-1 bg-transparent text-ink-soft border-0 cursor-pointer hover:text-ink"
                    >
                      Cancel
                    </button>
                  </div>
                </div>
              )}

              {/* Add buttons + drop hint */}
              {!addingMemory && (
                <div className="flex flex-col gap-1 mt-1">
                  <button
                    type="button"
                    onClick={openTextMemory}
                    className="text-[13px] text-ink-soft hover:text-ink bg-transparent border-0 p-0 text-left cursor-pointer transition-colors"
                  >
                    + Add note
                  </button>

                  <button
                    type="button"
                    onClick={() => fileInputRef.current?.click()}
                    disabled={fileLoading}
                    className="text-[13px] text-ink-soft hover:text-ink bg-transparent border-0 p-0 text-left cursor-pointer transition-colors disabled:opacity-50"
                  >
                    {fileLoading ? "Reading file…" : "📎 Add from file"}
                  </button>

                  <p className="text-ink-soft text-[11px] mt-1 leading-snug">
                    Or drag a file anywhere onto the sidebar.
                  </p>
                </div>
              )}

              {/* Hidden file input */}
              <input
                ref={fileInputRef}
                type="file"
                accept=".txt,.md,.pdf,.csv,.json,.py,.js,.ts,.rs,.html,.xml,.log,.yaml,.toml,.ini"
                className="hidden"
                onChange={handleFileInputChange}
              />
            </div>
          )}
        </>
      )}
    </aside>
  );
}
