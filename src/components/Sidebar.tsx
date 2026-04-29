import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ConversationSummary {
  id: string;
  title: string;
  updated_at: number;
}

interface Props {
  onNewChat?: () => void;
  activeConversationId: string | null;
  onSelectConversation: (id: string) => void;
  refreshKey: number;
}

export default function Sidebar({
  onNewChat,
  activeConversationId,
  onSelectConversation,
  refreshKey,
}: Props) {
  const [collapsed, setCollapsed] = useState(false);
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);

  // Reload the conversation list whenever refreshKey changes (new conversation
  // created or existing one updated) or on first mount.
  useEffect(() => {
    invoke<ConversationSummary[]>("list_conversations")
      .then(setConversations)
      .catch((e) => console.error("list_conversations failed:", e));
  }, [refreshKey]);

  return (
    <aside
      className="flex flex-col h-full bg-[#F0ECE3] border-r border-[#DDD8CE] shrink-0 transition-all duration-150 ease-out"
      style={{ width: collapsed ? "56px" : "260px" }}
    >
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
          <div className="px-4">
            <button
              type="button"
              onClick={onNewChat}
              className="bg-transparent text-ink-soft hover:text-ink border-0 p-0 text-[13px] text-left cursor-pointer transition-colors duration-150 ease-out"
            >
              + New chat
            </button>
          </div>

          <div className="text-ink-soft uppercase tracking-[2px] text-[10px] pt-5 px-4 pb-2">
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
                  className={`text-ink text-[13px] py-2 px-3 rounded-sm cursor-pointer transition-colors duration-150 ease-out truncate ${
                    activeConversationId === conv.id
                      ? "bg-[#E2DED5]"
                      : "hover:bg-[#E8E4DB]"
                  }`}
                  title={conv.title}
                >
                  {conv.title}
                </div>
              ))
            )}
          </nav>

          <div className="text-ink hover:text-forest cursor-pointer text-[14px] px-4 pt-4 pb-[20px] transition-colors duration-150 ease-out">
            <span className="mr-2">⚙️</span>
            Settings
          </div>
        </>
      )}
    </aside>
  );
}
