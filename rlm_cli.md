# Projektplan: RLM-CLI (The Context Broker)
===========================================

1\. Vision & Zieldefinition
---------------------------

**Das Problem:** Moderne KI-Agenten leiden unter "Context Rot". Wenn man ihnen ganze Dateien oder gar ganze Repositories in den Kontext lädt, verlieren sie den Fokus, werden langsam und machen Fehler.

**Die Lösung:** Wir entwickeln `rlm-cli`, ein hochperformantes CLI-Tool in Rust. Es fungiert als **intelligenter Bibliothekar** zwischen dem Dateisystem und dem KI-Agenten. Anstatt Dateien blind zu lesen, nutzt der Agent dieses Tool, um die Codebasis semantisch zu erforschen, gezielt Informationen abzurufen und chirurgische Änderungen vorzunehmen.

**Kernziele:**

1.  **Progressive Disclosure:** Der Agent sieht erst die Struktur (`tree`), dann die Suche (`search`), dann den Inhalt (`read`).
    
2.  **Surgical Editing:** Code-Änderungen (`replace`, `insert`) erfolgen präzise im AST (Abstract Syntax Tree), ohne die Formatierung der restlichen Datei zu zerstören.
    
3.  **Safety First:** Integrierte Syntax-Prüfung verhindert, dass der Agent Code speichert, der nicht kompiliert (z.B. fehlende Klammern).
    
4.  **Polyglot:** Unterstützung für Backend (Rust, Go, Java, C#), Frontend (TS, HTML, CSS), Skripte (Python, Bash) und Dokumentation (PDF, Markdown).
    

* * *

2\. Tech Stack
--------------

Wir setzen auf **Rust** für maximale Geschwindigkeit, Typsicherheit und einfache Verteilung (Single Binary).

*   **Sprache:** Rust
    
*   **CLI Interface:** `clap` (Feature `derive` für typisierte Argumente).
    
*   **Parallelisierung:** `rayon` (Data Parallelism für extrem schnelles Indexieren).
    
*   **Parsing Engine:** `tree-sitter` (Industriestandard für Code-Analyse).
    
*   **Datenbank:** `rusqlite` (SQLite mit `bundled` Feature). Wir nutzen **FTS5** (Full Text Search) für die Suche.
    
*   **Dateisystem:** `walkdir` (Rekursives Scannen), `ignore` (Beachtung von `.gitignore`).
    
*   **PDF Verarbeitung:** `pdf-extract` (Extraktion von Text aus Binärdaten).
    
*   **Hashing:** `sha2` (Erkennen von Dateiänderungen, um unnötiges Re-Indizieren zu vermeiden).
    

* * *

3\. Feature-Übersicht & Architektur
-----------------------------------

### Modul 1: The Ingestor (Datenaufnahme)

Das Herzstück des Systems. Es muss tausende Dateien in Sekunden scannen.

*   **Parallelität:** Nutzt alle CPU-Kerne via `rayon`.
    
*   **Router:** Entscheidet anhand der Dateiendung:
    
    *   _Code:_ Tree-sitter Parser laden.
        
    *   _PDF:_ Binär-Extraktion + Seiten-Splitting.
        
    *   _Markdown:_ Sektions-Splitting.
        
*   **Chunking:** Zerlegt Dateien in logische Blöcke (Funktionen, Klassen, Kapitel).
    
*   **UI-Awareness:** Erkennt anhand von Pfaden (`/pages/`, `.tsx`), ob Code zu einem UI-Screen gehört und taggt dies (`ui_context`).
    

### Modul 2: The Librarian (Datenbank & Suche)

*   **Speicher:** Lokale `.rlm/index.db` SQLite Datei.
    
*   **Smart Search:** FTS5 ermöglicht Suche nach Symbolen ("AuthService") und Volltext ("TODO: Fix this hack").
    
*   **Tree View:** Generiert dynamische "Landkarten" der Ordnerstruktur, angereichert mit Metadaten (z.B. "auth.rs \[Class: UserSession\]").
    

### Modul 3: The Surgeon (Manipulation)

Erlaubt dem Agenten, Code zu schreiben.

*   **Replace:** Tauscht einen Knoten (z.B. eine Funktion) komplett aus.
    
*   **Insert:** Fügt Code in Container (Klassen, Impl-Blöcke) ein – entweder oben, unten, vor oder nach einer spezifizierten Zeile.
    
*   **Syntax Guard:** **Vor** dem Speichern wird der neue Datei-String im RAM geparst. Wenn `tree.has_error()` wahr ist, wird das Speichern verweigert und der Fehler zurückgegeben.
    

* * *

4\. Unterstützte Sprachen & Formate
-----------------------------------

Das Tool muss folgende Parser integrieren:

| Kategorie | Sprachen / Formate | Tree-sitter Crate |
| --- | --- | --- |
| System | Rust, C, C++ | tree-sitter-rust,~-c,~-cpp |
| Enterprise | Java, C#, Go, PHP | ~-java,~-c-sharp,~-go,~-php |
| Scripting | Python, Bash | ~-python,~-bash(wenn verfügbar) |
| Web | JS, TS, HTML, CSS | ~-javascript,~-typescript,~-html,~-css |
| Data | JSON, SQL, YAML | ~-json, (SQL/YAML via Text-Heuristik in V1) |
| Docs | Markdown, PDF | ~-markdown,pdf-extract(Crate) |

* * *

5\. Implementierungs-Roadmap
----------------------------

### Phase 1: Setup & Data Layer

1.  `cargo init` und Dependencies konfigurieren.
    
2.  Datenbank-Schema erstellen (`files` und `chunks` Tabellen).
    
3.  `models.rs` definieren (`struct Chunk`).
    

### Phase 2: Der Indexer (MVP)

1.  Implementierung von `walkdir` + `rayon`.
    
2.  Einbau des "Dispatcher Patterns": Eine Funktion, die basierend auf der Extension den richtigen Parser wählt.
    
3.  Implementierung von 2-3 Referenz-Parsern (z.B. Rust und Python) mit echten Tree-sitter Queries.
    
4.  Implementierung des PDF-Extractors.
    

### Phase 3: Read & Search CLI

1.  Befehl `rlm index` finalisieren.
    
2.  Befehl `rlm search` (SQL Query gegen FTS5).
    
3.  Befehl `rlm read` (Dateizugriff).
    
4.  Befehl `rlm tree` (Die rekursive Ansicht aus der DB).
    

### Phase 4: Write Operations (High Complexity)

1.  Implementierung der Logik für `rlm replace` (Byte-Range Replacement).
    
2.  Implementierung von `rlm insert` (Container-Body Finding).
    
3.  Einbau des **Syntax Guard** (Validierung vor Write).
    

### Phase 5: Skalierung & Polish

1.  Hinzufügen aller restlichen Sprachen (C#, Java, etc.) durch Copy-Paste der Query-Logik.
    
2.  Befehl `rlm stats` hinzufügen.
    
3.  Error Handling verbessern (`anyhow`).
    

* * *

6\. Der "Master Prompt" für die Entwicklung
-------------------------------------------

Dies ist der Prompt, den Sie verwenden, um den Code generieren zu lassen. Er enthält alle oben genannten Anforderungen.

* * *

**System:** Du bist ein Senior Rust Developer und Architekt für Developer Tools.

**Auftrag:** Wir bauen `rlm-cli` – ein Tool für KI-Agenten zum semantischen Lesen und Schreiben von Code.

**Schritt 1: Cargo.toml & Setup** Erstelle ein neues Rust-Projekt. Füge folgende Dependencies hinzu (nutze aktuelle Versionen):

*   `clap` (features: derive), `anyhow`, `console`.
    
*   `rayon` (für Parallelisierung), `walkdir`, `ignore`, `sha2`.
    
*   `rusqlite` (features: bundled), `pdf-extract`.
    
*   `tree-sitter` (0.20+) und die Sprach-Pakete: `tree-sitter-rust`, `tree-sitter-javascript`, `tree-sitter-typescript`, `tree-sitter-python`, `tree-sitter-go`, `tree-sitter-java`, `tree-sitter-c-sharp`, `tree-sitter-cpp`, `tree-sitter-php`, `tree-sitter-markdown`, `tree-sitter-html`, `tree-sitter-css`, `tree-sitter-json`.
    

**Schritt 2: Das Datenmodell** Erstelle `src/db.rs`. Initialisiere eine SQLite DB mit:

*   Table `files`: id, path (unique), hash.
    
*   Table `chunks`: id, file\_id, start\_line, end\_line, kind (z.B. "fn"), identifier (Name), ui\_context (z.B. "LoginScreen"), content.
    
*   FTS5 Virtual Table für Volltextsuche.
    

**Schritt 3: Der Parallele Ingestor** Implementiere `rlm index`.

*   Nutze `rayon` (`par_iter`), um Dateien parallel zu verarbeiten.
    
*   Schreibe einen `Dispatcher`, der je nach Dateiendung den passenden Parser wählt.
    
*   **PDF:** Nutze `pdf-extract`, splitte nach Seiten.
    
*   **Code:** Nutze `tree-sitter`. Extrahiere vorerst generisch "Blöcke" (Funktionen/Klassen).
    
*   **UI-Erkennung:** Wenn der Pfad UI-Begriffe enthält, setze das `ui_context` Feld.
    

**Schritt 4: Die "Chirurgen"-Features (Write)** Implementiere `rlm replace` und `rlm insert`.

*   Nutze Tree-sitter, um die Byte-Positionen der Knoten zu finden.
    
*   **WICHTIG:** Implementiere einen **Syntax Guard**. Bevor du schreibst, parse den neuen String im RAM. Wenn `has_error()` true ist, brich ab und gib den Fehler aus.
    

**Schritt 5: Orientierung** Implementiere `rlm tree` und `rlm stats`.

*   `tree` soll die Ordnerstruktur aus der DB anzeigen, angereichert mit den wichtigsten Symbolen pro Datei (z.B. `auth.rs [fn: login]`).
    

**Ziel:** Der Code muss kompilieren und eine solide Architektur für alle genannten Sprachen bieten. Fange mit der `main.rs` Struktur und dem Datenbank-Setup an.

* * *

---
