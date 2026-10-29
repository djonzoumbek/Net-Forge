import { useEffect, useRef, useState, type KeyboardEvent } from "react";

export interface SearchOptions {
  caseSensitive: boolean;
  regex: boolean;
}

interface SearchBarProps {
  /** Changes on each Ctrl+Shift+F: gives the focus back to the field. */
  focusToken: number;
  /** Returns false when nothing matches. */
  onSearch: (text: string, options: SearchOptions, backwards: boolean) => boolean;
  onClose: () => void;
}

/** Search in the scrollback of one terminal (Enter: next, Shift+Enter: previous). */
export function SearchBar({ focusToken, onSearch, onClose }: SearchBarProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [text, setText] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [regex, setRegex] = useState(false);
  const [notFound, setNotFound] = useState(false);

  useEffect(() => {
    inputRef.current?.select();
  }, [focusToken]);

  function search(backwards: boolean) {
    if (!text) return;
    setNotFound(!onSearch(text, { caseSensitive, regex }, backwards));
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Enter") {
      event.preventDefault();
      search(event.shiftKey);
    } else if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }

  return (
    <div className="search-bar" role="search">
      <input
        ref={inputRef}
        autoFocus
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          setNotFound(false);
        }}
        onKeyDown={onKeyDown}
        placeholder="Rechercher dans l'historique"
        aria-label="Texte recherché"
        aria-invalid={notFound}
        spellCheck={false}
      />
      <label className="checkbox" title="Respecter la casse">
        <input type="checkbox" checked={caseSensitive} onChange={(e) => setCaseSensitive(e.target.checked)} />
        Aa
      </label>
      <label className="checkbox" title="Expression régulière (JavaScript)">
        <input type="checkbox" checked={regex} onChange={(e) => setRegex(e.target.checked)} />
        .*
      </label>
      <button onClick={() => search(true)} title="Précédent (Maj+Entrée)" aria-label="Précédent">
        ↑
      </button>
      <button onClick={() => search(false)} title="Suivant (Entrée)" aria-label="Suivant">
        ↓
      </button>
      {notFound && <span className="search-status">Aucun résultat</span>}
      <span className="spacer" />
      <button onClick={onClose} title="Fermer (Échap)" aria-label="Fermer la recherche">
        ×
      </button>
    </div>
  );
}
