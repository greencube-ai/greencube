import { useEffect, useState } from "react";
import Sidebar from "./components/Sidebar";
import MainArea from "./components/MainArea";
import SplashScreen from "./components/SplashScreen";

export default function App() {
  const [splashVisible, setSplashVisible] = useState(true);
  const [splashFading, setSplashFading] = useState(false);
  const [chatKey, setChatKey] = useState(0);

  useEffect(() => {
    const fadeTimer = setTimeout(() => setSplashFading(true), 3000);
    const removeTimer = setTimeout(() => setSplashVisible(false), 3500);
    return () => {
      clearTimeout(fadeTimer);
      clearTimeout(removeTimer);
    };
  }, []);

  return (
    <div className="h-screen flex">
      <Sidebar onNewChat={() => setChatKey((k) => k + 1)} />
      <MainArea key={chatKey} />
      {splashVisible && <SplashScreen fadingOut={splashFading} />}
    </div>
  );
}
