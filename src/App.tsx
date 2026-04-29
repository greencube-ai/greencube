import { useEffect, useState } from "react";
import Sidebar from "./components/Sidebar";
import MainArea from "./components/MainArea";
import SplashScreen from "./components/SplashScreen";

export default function App() {
  const [splashVisible, setSplashVisible] = useState(true);
  const [splashFading, setSplashFading] = useState(false);

  // Tracks which conversation is highlighted in the sidebar.
  const [activeConversationId, setActiveConversationId] = useState<string | null>(null);
  // Increments ONLY on explicit user navigation (new chat / click conversation).
  // MainArea uses this as its key so it remounts only when the user navigates,
  // NOT when a conversation is created mid-stream (which would kill streaming).
  const [navigationKey, setNavigationKey] = useState(0);
  // Increments to tell the Sidebar to refresh its conversation list.
  const [sidebarRefreshKey, setSidebarRefreshKey] = useState(0);

  useEffect(() => {
    const fadeTimer = setTimeout(() => setSplashFading(true), 3000);
    const removeTimer = setTimeout(() => setSplashVisible(false), 3500);
    return () => {
      clearTimeout(fadeTimer);
      clearTimeout(removeTimer);
    };
  }, []);

  function handleNewChat() {
    setActiveConversationId(null);
    setNavigationKey((k) => k + 1); // remount MainArea → fresh state
  }

  function handleSelectConversation(id: string) {
    setActiveConversationId(id);
    setNavigationKey((k) => k + 1); // remount MainArea → load that conversation
  }

  // Called by MainArea when a new conversation row is first created in the DB.
  // Must NOT increment navigationKey — MainArea is already streaming at this point.
  function handleConversationCreated(id: string) {
    setActiveConversationId(id);      // update sidebar highlight
    setSidebarRefreshKey((k) => k + 1); // refresh sidebar list
  }

  function handleConversationUpdated() {
    setSidebarRefreshKey((k) => k + 1);
  }

  return (
    <div className="h-screen flex">
      <Sidebar
        onNewChat={handleNewChat}
        activeConversationId={activeConversationId}
        onSelectConversation={handleSelectConversation}
        refreshKey={sidebarRefreshKey}
      />
      <MainArea
        key={navigationKey}
        conversationId={activeConversationId}
        onConversationCreated={handleConversationCreated}
        onConversationUpdated={handleConversationUpdated}
      />
      {splashVisible && <SplashScreen fadingOut={splashFading} />}
    </div>
  );
}
