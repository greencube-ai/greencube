import { useState } from "react";

export default function Sidebar() {
  const [collapsed, setCollapsed] = useState(false);

  return (
    <aside
      className="flex flex-col h-full bg-[#F0ECE3] border-r border-[#DDD8CE] shrink-0 transition-all duration-150 ease-out"
      style={{ width: collapsed ? "56px" : "260px" }}
    >
      <div className="flex items-center justify-between p-4">
        {!collapsed && (
          <span
            className="text-forest"
            style={{
              fontFamily: "Georgia, serif",
              fontWeight: "bold",
              fontSize: "16px",
            }}
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
              className="bg-transparent text-ink-soft hover:text-ink border-0 p-0 text-[13px] text-left cursor-pointer transition-colors duration-150 ease-out"
            >
              + New chat
            </button>
          </div>

          <div className="text-ink-soft uppercase tracking-[2px] text-[10px] pt-5 px-4 pb-2">
            RECENT
          </div>

          <div className="flex-1 flex items-center justify-center text-ink-soft text-[13px] px-4">
            No conversations yet
          </div>

          <div className="text-ink hover:text-forest cursor-pointer text-[14px] px-4 pt-4 pb-[20px] transition-colors duration-150 ease-out">
            <span className="mr-2">⚙️</span>
            Settings
          </div>
        </>
      )}
    </aside>
  );
}
