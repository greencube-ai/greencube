import { useEffect, useState } from "react";

const SUGGESTIONS = [
  "Organize my photos by date",
  "Build me a todo app",
  "Rewrite my essay in my style",
];

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

export default function MainArea() {
  const [inputValue, setInputValue] = useState("");
  const [phraseIndex, setPhraseIndex] = useState(0);
  const [phraseVisible, setPhraseVisible] = useState(false);

  useEffect(() => {
    setPhraseVisible(true);
    const fadeOut = setTimeout(() => setPhraseVisible(false), 3300);
    const swap = setTimeout(() => {
      setPhraseIndex((i) => (i + 1) % PHRASES.length);
    }, 3600);
    return () => {
      clearTimeout(fadeOut);
      clearTimeout(swap);
    };
  }, [phraseIndex]);

  return (
    <main className="flex-1 flex items-center justify-center bg-cream px-6">
      <div className="w-full max-w-[720px] flex flex-col items-center">
        <h1
          className="text-forest mb-7"
          style={{
            fontFamily: "Georgia, serif",
            fontWeight: "bold",
            fontSize: "48px",
          }}
        >
          Create.
        </h1>

        <div className="relative w-full mb-5">
          <input
            type="text"
            value={inputValue}
            onChange={(e) => setInputValue(e.target.value)}
            className="w-full h-12 pl-4 pr-14 bg-white text-ink text-[15px] border-[1.5px] border-[#DDD8CE] rounded-lg outline-none"
          />
          <button
            type="button"
            aria-label="Send"
            className="absolute top-[6px] right-[6px] w-[36px] h-[36px] rounded-full bg-forest hover:bg-moss text-white text-[18px] flex items-center justify-center cursor-pointer border-0 transition-colors duration-150 ease-out"
          >
            →
          </button>
          {inputValue === "" && (
            <div
              className="absolute inset-0 flex items-center pointer-events-none text-ink-soft text-[15px]"
              style={{
                paddingLeft: "16px",
                paddingRight: "56px",
                opacity: phraseVisible ? 1 : 0,
                transition: "opacity 300ms ease-out",
              }}
            >
              {PHRASES[phraseIndex]}
            </div>
          )}
        </div>

        <div className="flex gap-3 mb-6 justify-center">
          {SUGGESTIONS.map((chip) => (
            <button
              key={chip}
              type="button"
              className="shrink-0 whitespace-nowrap cursor-pointer bg-transparent border-[1.5px] border-[#C8C3B8] text-ink rounded-[20px] py-2 px-4 text-[14px] transition-colors duration-150 ease-out hover:bg-forest hover:text-white hover:border-forest"
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
