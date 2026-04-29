import { useEffect, useState } from "react";

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
      <div className="w-full max-w-[600px] flex flex-col items-center">
        <h1
          className="text-forest mb-6"
          style={{
            fontFamily: "Georgia, serif",
            fontWeight: "bold",
            fontSize: "48px",
          }}
        >
          Create.
        </h1>

        <div className="relative w-full mb-4">
          <input
            type="text"
            value={inputValue}
            onChange={(e) => setInputValue(e.target.value)}
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
