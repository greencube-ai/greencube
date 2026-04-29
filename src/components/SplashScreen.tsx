type Props = { fadingOut: boolean };

export default function SplashScreen({ fadingOut }: Props) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-cream transition-opacity duration-500 ease-out"
      style={{
        opacity: fadingOut ? 0 : 1,
        pointerEvents: fadingOut ? "none" : "auto",
      }}
    >
      <h1
        className="text-forest"
        style={{
          fontFamily: "Georgia, serif",
          fontWeight: "bold",
          fontSize: "64px",
        }}
      >
        GreenCube
      </h1>
    </div>
  );
}
