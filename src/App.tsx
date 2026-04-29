import { useEffect, useState } from "react";
import Sidebar from "./components/Sidebar";
import MainArea from "./components/MainArea";
import SplashScreen from "./components/SplashScreen";

export default function App() {
  const [splashVisible, setSplashVisible] = useState(true);
  const [splashFading, setSplashFading] = useState(false);

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
      <Sidebar />
      <MainArea />
      {splashVisible && <SplashScreen fadingOut={splashFading} />}
    </div>
  );
}