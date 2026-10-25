//! The interface in the user's language.
//!
//! Every user-visible string lives in one table ([`Text`]), written in English and Italian.
//! The table is generated from a single list so that adding a string without translating it
//! does not compile. Dynamic sentences keep named placeholders (`{n}`, `{time}`…) that
//! [`fill`] substitutes; the tests check that both languages use the same placeholders.
//!
//! The choice is stored in `ui.json` next to the job list, written atomically, and never
//! leaves the machine. A missing or unreadable file means English: a preference must never
//! stop the app from opening.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The languages the interface speaks. English is the default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Lang {
    #[default]
    English,
    Italiano,
}

impl Lang {
    /// Every language, in the order the selector lists them.
    pub(crate) const ALL: [Lang; 2] = [Lang::English, Lang::Italiano];

    /// The language's name in that language, for the selector.
    pub(crate) fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Italiano => "Italiano",
        }
    }

    /// The short code written to `ui.json`.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Italiano => "it",
        }
    }

    /// Reads a saved code. Accepts `it`, `it-IT`, `Italian`, `italiano`…; `None` for what we
    /// do not know, so a file from another version still opens in English.
    pub(crate) fn from_code(code: &str) -> Option<Self> {
        let base = code.trim().split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase();
        match base.as_str() {
            "en" | "english" => Some(Self::English),
            "it" | "italian" | "italiano" => Some(Self::Italiano),
            _ => None,
        }
    }
}

/// The language choice as stored in `ui.json`. Only the language: no personal data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Preferences {
    pub(crate) lang: Lang,
}

#[derive(Debug, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    language: String,
}

impl Preferences {
    /// Reads a preferences file. A missing or damaged file means English, never an error:
    /// a language choice must not keep the app from starting.
    pub(crate) fn load(path: &Path) -> Self {
        let Ok(bytes) = std::fs::read(path) else { return Self::default() };
        Self::from_json(&bytes)
    }

    pub(crate) fn from_json(bytes: &[u8]) -> Self {
        serde_json::from_slice::<Stored>(bytes).ok().and_then(|s| Lang::from_code(&s.language)).map_or(Self::default(), |lang| Self { lang })
    }

    pub(crate) fn to_json(self) -> Result<Vec<u8>, String> {
        serde_json::to_vec_pretty(&Stored { language: self.lang.code().to_owned() }).map_err(|e| e.to_string())
    }

    /// Writes the choice atomically, so a crash never leaves half a file behind.
    pub(crate) fn save(self, path: &Path) -> Result<(), String> {
        let json = self.to_json()?;
        scribecraft_engine::write_atomic(path, &json).map_err(|e| e.to_string())
    }
}

/// Substitutes `{name}` placeholders with their values in one pass. The template is a static
/// string and the values are the user's data, so this cannot fail; a value is copied in
/// literally and never scanned again, even when it contains braces.
pub(crate) fn fill(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    loop {
        let Some(open) = rest.find('{') else {
            out.push_str(rest);
            return out;
        };
        let Some(end) = rest.get(open..).and_then(|tail| tail.find('}')).filter(|end| *end > 1).map(|end| open + end) else {
            out.push_str(rest);
            return out;
        };
        out.push_str(rest.get(..open).unwrap_or_default());
        let name = rest.get(open..=end).unwrap_or_default();
        match args.iter().find(|(key, _)| *key == name) {
            Some((_, value)) => out.push_str(value),
            None => out.push_str(name),
        }
        rest = rest.get(end + 1..).unwrap_or_default();
    }
}

/// What the toolbar says after a multi-file pick or a folder scan.
pub(crate) fn added_recordings(n: usize, lang: Lang) -> String {
    let text = if n == 1 { Text::AddedOne } else { Text::AddedMany };
    fill(text.in_lang(lang), &[("{n}", &n.to_string())])
}

/// How many jobs the queue holds.
pub(crate) fn in_queue(n: usize, lang: Lang) -> String {
    fill(Text::InQueue.in_lang(lang), &[("{n}", &n.to_string())])
}

/// How many system-wide hotkeys are registered (singular and plural read differently).
pub(crate) fn hotkey_count(n: usize, lang: Lang) -> String {
    let text = if n == 1 { Text::HotkeySystemOne } else { Text::HotkeysSystem };
    fill(text.in_lang(lang), &[("{n}", &n.to_string())])
}

/// The report after copying a time stamp to the clipboard.
pub(crate) fn copied(text: &str, lang: Lang) -> String {
    fill(Text::Copied.in_lang(lang), &[("{text}", text)])
}

/// The report after marking a bookmark.
pub(crate) fn bookmark_at(time: &str, lang: Lang) -> String {
    fill(Text::BookmarkAt.in_lang(lang), &[("{time}", time)])
}

/// The error for a dropped file that is not a recording.
pub(crate) fn not_supported(path: &str, lang: Lang) -> String {
    fill(Text::NotSupported.in_lang(lang), &[("{path}", path)])
}

/// The startup error when the data folder cannot be created.
pub(crate) fn cannot_create(dir: &str, error: &str, lang: Lang) -> String {
    fill(Text::CannotCreate.in_lang(lang), &[("{dir}", dir), ("{error}", error)])
}

/// The hint under a scan button, naming its rate.
pub(crate) fn rewind_hint(rate: f64, lang: Lang) -> String {
    fill(Text::RewindHint.in_lang(lang), &[("{rate}", &format!("{rate:.0}"))])
}

pub(crate) fn forward_hint(rate: f64, lang: Lang) -> String {
    fill(Text::ForwardHint.in_lang(lang), &[("{rate}", &format!("{rate:.0}"))])
}

macro_rules! texts {
    ($($variant:ident => $english:literal | $italian:literal),+ $(,)?) => {
        /// Every user-visible string, in both languages.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum Text {
            $($variant),+
        }

        impl Text {
            /// Every string, for the tests that check the table is complete.
            #[cfg(test)]
            pub(crate) const ALL: &'static [Text] = &[$(Text::$variant),+];

            /// The string in `lang`.
            pub(crate) fn in_lang(self, lang: Lang) -> &'static str {
                match lang {
                    Lang::English => self.english(),
                    Lang::Italiano => self.italian(),
                }
            }

            fn english(self) -> &'static str {
                match self {
                    $(Text::$variant => $english),+
                }
            }

            fn italian(self) -> &'static str {
                match self {
                    $(Text::$variant => $italian),+
                }
            }
        }
    };
}

texts! {
    // Toolbar and the reports it shows.
    AddRecordings => "Add recordings…" | "Aggiungi registrazioni…",
    AddRecordingsHint => "Choose one or more recordings to add to the job list" | "Scegli una o più registrazioni da aggiungere alla coda",
    RecordingsFilter => "Recordings" | "Registrazioni",
    AddFolder => "Add folder…" | "Aggiungi cartella…",
    Save => "Save" | "Salva",
    PedalAndKeys => "Pedal and keys…" | "Pedale e tasti…",
    ToolbarKeys => "F10 stamp · Ctrl+B bookmark" | "F10 tempo · Ctrl+B segnalibro",
    Everywhere => "everywhere" | "ovunque",
    InThisWindow => "in this window" | "in questa finestra",
    LanguageHint => "Interface language" | "Lingua dell'interfaccia",

    // Queue headings and empty states.
    Dictations => "Dictations" | "Dettature",
    InQueue => "{n} in queue" | "{n} in coda",
    NoDictations => "No dictations yet. Use “Add recordings…” or drop files on the window." | "Nessuna dettatura. Usa «Aggiungi registrazioni…» o trascina i file nella finestra.",
    NothingUnder => "Nothing under “{filter}”. Pick “{all}” to see the whole queue." | "Niente sotto «{filter}». Scegli «{all}» per vedere tutta la coda.",
    NoOpen => "No dictation open" | "Nessuna dettatura aperta",
    AddOrOpen => "Add or open a recording to start transcribing." | "Aggiungi o apri una registrazione per iniziare a trascrivere.",
    PickFromQueue => "Pick one from the queue, or add a recording to start." | "Scegline una dalla coda, o aggiungi una registrazione per iniziare.",
    Untitled => "Untitled recording" | "Dettatura senza titolo",
    NoAuthor => "no author" | "senza autore",

    // Filters.
    FilterAll => "All" | "Tutte",
    FilterTodo => "To do" | "Da fare",
    FilterInProgress => "In progress" | "In corso",
    FilterDone => "Done" | "Completate",
    FilterSent => "Sent" | "Inviate",

    // Job states.
    StatusNew => "new" | "nuova",
    StatusInProgress => "in progress" | "in corso",
    StatusDone => "done" | "completata",
    StatusDispatched => "dispatched" | "inviata",

    // Priorities.
    PriorityLow => "low" | "bassa",
    PriorityNormal => "normal" | "normale",
    PriorityHigh => "high" | "alta",
    PriorityUrgent => "urgent" | "urgente",

    // Playback settings.
    Playback => "PLAYBACK" | "RIPRODUZIONE",
    Volume => "volume" | "volume",
    Channels => "channels" | "canali",
    ChannelStereo => "both" | "entrambi",
    ChannelLeft => "left only" | "solo sinistra",
    ChannelRight => "right only" | "solo destra",
    ChannelMix => "mono mix" | "mix mono",
    AutoBackstep => "auto-backstep" | "passo indietro",
    SlowField => "slow" | "lenta",
    FastField => "fast" | "veloce",

    // The details card.
    Dictation => "DICTATION" | "DETTATURA",
    Title => "Title" | "Titolo",
    Author => "Author" | "Autore",
    WhoDictated => "who dictated it" | "chi l'ha dettata",
    Priority => "Priority" | "Priorità",
    Status => "Status" | "Stato",
    Due => "Due" | "Scadenza",
    DueFormat => "YYYY-MM-DD" | "AAAA-MM-GG",
    Bookmarks => "BOOKMARKS" | "SEGNALIBRI",
    BookmarkLabel => "label" | "etichetta",
    Add => "Add" | "Aggiungi",
    CopyTime => "Copy time" | "Copia tempo",
    CopyTimeHint => "Copy the time stamp (F10)" | "Copia la marca temporale (F10)",
    NoBookmarks => "None yet. Ctrl+B marks the playhead." | "Nessuno. Ctrl+B segna il punto di riproduzione.",
    JumpHere => "Jump here" | "Vai qui",

    // Notes and the open recording.
    Notes => "NOTES" | "NOTE",
    NotesHint => "Anything to remember while transcribing" | "Qualcosa da ricordare durante la trascrizione",
    Recording => "RECORDING" | "REGISTRAZIONE",

    // The status bar.
    Unsaved => "unsaved changes" | "modifiche non salvate",
    Ready => "ready" | "pronto",

    // The transport dock.
    ScanHint => "Hold a scan button to scan; tap it to skip." | "Tieni premuto un pulsante di scansione per scorrere; un tocco salta.",
    GoToStart => "Go to the start" | "Vai all'inizio",
    SkipBack => "Skip back" | "Indietro",
    SkipForward => "Skip forward" | "Avanti",
    RewindHint => "Rewind at {rate}× — hold to scan, tap to skip back (F7)" | "Riavvolgi a {rate}× — tieni premuto per scorrere, tocca per tornare indietro (F7)",
    ForwardHint => "Fast-forward at {rate}× — hold to scan, tap to skip forward (F8)" | "Avanti veloce a {rate}× — tieni premuto per scorrere, tocca per saltare avanti (F8)",
    Play => "Play" | "Riproduci",
    Stop => "Stop" | "Ferma",
    PlayHint => "Play (F9)" | "Riproduci (F9)",
    StopHint => "Stop (F9)" | "Ferma (F9)",
    Length => "Length of the dictation" | "Durata della dettatura",
    TimeRemaining => "Time remaining" | "Tempo rimanente",
    Stopped => "stopped" | "fermo",
    Playing => "playing" | "in riproduzione",
    Rewinding => "rewinding {rate}×" | "riavvolgimento {rate}×",
    FastForwarding => "fast-forwarding {rate}×" | "avanti veloce {rate}×",
    SlowPreset => "Slow" | "Lenta",
    FastPreset => "Fast" | "Veloce",
    SlowPresetHint => "Slow preset (F5)" | "Velocità lenta (F5)",
    FastPresetHint => "Fast preset (F6)" | "Velocità veloce (F6)",
    ResetSpeed => "Reset to normal speed" | "Torna alla velocità normale",

    // The "Pedal and keys" window.
    PedalAndKeysTitle => "Pedal and keys" | "Pedale e tasti",
    FootPedalTab => "Foot pedal" | "Pedale",
    KeysTab => "Keys" | "Tasti",

    // The guided pedal setup.
    WizardOpen => "Set up a pedal…" | "Configura il pedale…",
    WizardTitle => "Set up a pedal" | "Configura il pedale",
    WizardStepOf => "Step {n} of 3" | "Passo {n} di 3",
    WizardDeviceStep => "1 · Device" | "1 · Dispositivo",
    WizardLearnStep => "2 · Learn" | "2 · Apprendi",
    WizardAssignStep => "3 · Functions" | "3 · Funzioni",
    WizardDeviceIntro => "Connect the pedal and choose it here. A serial pedal starts from the standard wiring; a USB pedal is learned." | "Collega il pedale e sceglilo qui. Un pedale seriale parte dal cablaggio standard; un pedale USB viene appreso.",
    WizardNoDevices => "No pedal found. Connect it, then press Refresh." | "Nessun pedale trovato. Collegalo, poi premi Aggiorna.",
    WizardLearnIntro => "Press each pedal once, in any order: the wizard remembers which is which. Then say what each one does." | "Premi ogni pedale una volta, in qualsiasi ordine: il wizard ricorda quale è quale. Poi scegli cosa fa ciascuno.",
    WizardPressFirst => "Press the first pedal to learn it." | "Premi il primo pedale per apprenderlo.",
    WizardAssignIntro => "Choose what each pedal does, then press it to check that it lights up." | "Scegli cosa fa ciascun pedale, poi premilo per verificare che si illumini.",
    WizardBack => "Back" | "Indietro",
    WizardNext => "Next" | "Avanti",
    WizardFinish => "Finish" | "Fine",
    WizardNeedDevice => "Choose a device to continue." | "Scegli un dispositivo per continuare.",
    WizardNeedPedal => "Learn at least one pedal to continue." | "Apprendi almeno un pedale per continuare.",
    PedalIntro => "1. Connect the pedal (USB or serial/COM port) and choose it. 2. For each pedal click Learn and press it. 3. Choose what it does." | "1. Collega il pedale (USB o seriale/COM) e sceglilo. 2. Per ogni pedale clicca Apprendi e premilo. 3. Scegli cosa fa.",
    Refresh => "Refresh" | "Aggiorna",
    NoPedal => "no pedal" | "nessun pedale",
    Device => "device" | "dispositivo",
    SerialPrefix => "Serial: " | "Seriale: ",
    ConnectedPress => "connected: press a pedal to see it light up below" | "collegato: premi un pedale per vederlo illuminarsi sotto",
    NoPedalSelected => "no pedal selected" | "nessun pedale selezionato",
    PlayPedal => "Play pedal:" | "Pedale di riproduzione:",
    PlayWhileHeld => "plays while held, stops when released" | "suona mentre è premuto, si ferma al rilascio",
    PlayToggleMode => "press to play, press again to stop" | "premi per riprodurre, premi di nuovo per fermare",
    PedalColumn => "Pedal" | "Pedale",
    FunctionColumn => "Function" | "Funzione",
    KeyColumn => "Key" | "Tasto",
    PedalNumber => "Pedal {n}" | "Pedale {n}",
    Pressed => "● pressed" | "● premuto",
    LearnAgain => "Learn again" | "Riapprendi",
    Remove => "remove" | "rimuovi",
    LearnNew => "Learn a new pedal" | "Apprendi un nuovo pedale",
    PressPedal => "Press pedal {n} now…" | "Premi ora il pedale {n}…",
    ReleasePedals => "Release every pedal first; learning starts when they are all up." | "Rilascia prima tutti i pedali; l'apprendimento inizia quando sono tutti sollevati.",
    CancelLearning => "Cancel learning" | "Annulla apprendimento",
    SerialWiring => "Standard serial wiring: pin 1 play, pin 6 rewind, pins 8/9 fast-forward" | "Cablaggio seriale standard: pin 1 riproduci, pin 6 riavvolgi, pin 8/9 avanti veloce",
    LastReport => "last report from the device: {report}" | "ultimo messaggio dal dispositivo: {report}",
    KeysGlobal => "Keys work while another program (e.g. the word processor) is in front" | "I tasti funzionano mentre un altro programma (es. il word processor) è in primo piano",
    KeysHelp => "Click Assign, then press the key or combination. Function keys work alone; other keys need Ctrl or Alt." | "Clicca Assegna, poi premi il tasto o la combinazione. I tasti funzione funzionano da soli; gli altri richiedono Ctrl o Alt.",
    PressKeyFor => "Press the key for “{action}” (Esc to cancel)…" | "Premi il tasto per «{action}» (Esc per annullare)…",
    PressAKey => "press a key…" | "premi un tasto…",
    Assign => "Assign…" | "Assegna…",
    Clear => "clear" | "rimuovi",
    RestoreDefaults => "Restore the default keys" | "Ripristina i tasti predefiniti",
    TooManyKeys => "too many keys; clear one first" | "troppi tasti; rimuovine uno prima",
    KeyMoved => "{key} now runs “{action}” (it no longer runs “{old}”)" | "{key} ora esegue «{action}» (non esegue più «{old}»)",
    KeyAssigned => "{key} now runs “{action}”" | "{key} ora esegue «{action}»",
    KeyCannotUse => "{key} cannot be used; choose a function key (F1-F20) or a letter, digit or arrow with Ctrl or Alt" | "{key} non può essere usato; scegli un tasto funzione (F1-F20) oppure una lettera, cifra o freccia con Ctrl o Alt",
    KeyNeedsModifier => "{key} alone would stop that key from typing; press it with Ctrl or Alt, or use a function key" | "{key} da solo impedirebbe di digitare quel tasto; premilo con Ctrl o Alt, oppure usa un tasto funzione",

    // What a pedal or a key does.
    ActionNone => "nothing" | "niente",
    ActionRewind => "rewind (hold)" | "riavvolgi (tieni premuto)",
    ActionPlay => "play" | "riproduci",
    ActionFastForward => "fast-forward (hold)" | "avanti veloce (tieni premuto)",
    ActionStop => "stop" | "ferma",
    ActionPlayToggle => "play / stop" | "riproduci / ferma",
    ActionSkipBack => "skip back" | "indietro",
    ActionSkipForward => "skip forward" | "avanti",
    ActionSlow => "slow speed" | "velocità lenta",
    ActionFast => "fast speed" | "velocità veloce",

    // Pedal and hotkey status lines.
    PedalNone => "pedal: none" | "pedale: nessuno",
    PedalConnected => "pedal: connected" | "pedale: collegato",
    PedalConnecting => "pedal: connecting" | "pedale: connessione",
    PedalWaiting => "pedal: waiting for the device" | "pedale: in attesa del dispositivo",
    PedalStopped => "pedal: stopped" | "pedale: fermo",
    HotkeySystemOne => "1 system-wide hotkey" | "1 scorciatoia di sistema",
    HotkeysSystem => "{n} system-wide hotkeys" | "{n} scorciatoie di sistema",
    HotkeysWindow => "hotkeys: window only" | "scorciatoie: solo finestra",

    // Reports.
    CannotCreate => "cannot create {dir}: {error}; the job list will not be saved" | "impossibile creare {dir}: {error}; la coda non verrà salvata",
    Copied => "copied {text}" | "copiato {text}",
    BookmarkAt => "bookmark at {time}" | "segnalibro a {time}",
    NotSupported => "{path} is not a supported recording" | "{path} non è una registrazione supportata",
    AddedOne => "added 1 recording" | "aggiunta 1 registrazione",
    AddedMany => "added {n} recordings" | "aggiunte {n} registrazioni",
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `{name}` placeholders a template uses, in order.
    fn placeholders(text: &str) -> Vec<&str> {
        text.split('{').skip(1).filter_map(|rest| rest.split('}').next()).collect()
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("scribecraft-ui-{}-{name}.json", std::process::id()))
    }

    #[test]
    fn every_string_is_translated_and_nobody_forgot_a_placeholder() {
        for &text in Text::ALL {
            let (en, it) = (text.english(), text.italian());
            assert!(!en.trim().is_empty(), "{text:?} has no English");
            assert!(!it.trim().is_empty(), "{text:?} has no Italian");
            assert_eq!(placeholders(en), placeholders(it), "{text:?} must use the same placeholders in both languages");
        }
    }

    /// A sentence that cannot be filled must not be shown with braces in it.
    #[test]
    fn filling_a_sentence_leaves_no_placeholders_behind() {
        for &text in Text::ALL {
            for lang in Lang::ALL {
                let template = text.in_lang(lang);
                let owned: Vec<(String, &str)> = placeholders(template).into_iter().map(|name| (format!("{{{name}}}"), "x")).collect();
                let args: Vec<(&str, &str)> = owned.iter().map(|(name, value)| (name.as_str(), *value)).collect();
                let filled = fill(template, &args);
                assert!(!filled.contains(['{', '}']), "{text:?} in {lang:?} left a placeholder: {filled}");
            }
        }
    }

    /// A user's value is copied in literally: a file name containing `{b}` must not be read
    /// as another placeholder.
    #[test]
    fn a_value_is_never_read_as_a_placeholder() {
        let out = fill("{a} and {b}", &[("{a}", "{b}"), ("{b}", "x")]);
        assert_eq!(out, "{b} and x");
        assert_eq!(fill("{a}", &[]), "{a}", "an unknown placeholder is left for the reader to see");
        assert_eq!(fill("plain", &[("{a}", "x")]), "plain");
    }

    /// A translation that is still the English text is a missed string.
    #[test]
    fn the_italian_table_is_not_just_english_copied_over() {
        // "volume" is the same word in both languages; nothing else may be.
        let same = [Text::Volume];
        for &text in Text::ALL {
            if same.contains(&text) {
                continue;
            }
            assert_ne!(text.english(), text.italian(), "{text:?} is not translated");
        }
    }

    #[test]
    fn english_labels_do_not_collide() {
        let mut seen = std::collections::BTreeSet::new();
        for &text in Text::ALL {
            assert!(seen.insert(text.english()), "two strings share the English label {:?}", text.english());
        }
    }

    #[test]
    fn a_saved_code_maps_to_a_language_and_an_unknown_one_to_english() {
        assert_eq!(Lang::from_code("it"), Some(Lang::Italiano));
        assert_eq!(Lang::from_code("IT"), Some(Lang::Italiano));
        assert_eq!(Lang::from_code("it-IT"), Some(Lang::Italiano));
        assert_eq!(Lang::from_code(" it_IT "), Some(Lang::Italiano));
        assert_eq!(Lang::from_code("italiano"), Some(Lang::Italiano));
        assert_eq!(Lang::from_code("en-GB"), Some(Lang::English));
        assert_eq!(Lang::from_code("fr"), None);
        assert_eq!(Lang::from_code(""), None);
    }

    #[test]
    fn a_missing_or_damaged_preference_file_means_english() {
        assert_eq!(Preferences::from_json(b"").lang, Lang::English);
        assert_eq!(Preferences::from_json(b"not json").lang, Lang::English);
        assert_eq!(Preferences::from_json(br#"{"language":"fr"}"#).lang, Lang::English);
        assert_eq!(Preferences::load(&temp_path("never-written")).lang, Lang::English);
    }

    #[test]
    fn a_language_choice_is_written_and_read_back() {
        let path = temp_path("language");
        let _ = std::fs::remove_file(&path);
        Preferences { lang: Lang::Italiano }.save(&path).unwrap();
        assert_eq!(Preferences::load(&path).lang, Lang::Italiano, "the chosen language must survive a restart");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_dynamic_sentences_name_their_numbers() {
        assert_eq!(added_recordings(1, Lang::English), "added 1 recording");
        assert_eq!(added_recordings(3, Lang::Italiano), "aggiunte 3 registrazioni");
        assert_eq!(in_queue(4, Lang::Italiano), "4 in coda");
        assert_eq!(hotkey_count(1, Lang::Italiano), "1 scorciatoia di sistema");
        assert_eq!(hotkey_count(5, Lang::English), "5 system-wide hotkeys");
        assert_eq!(bookmark_at("00:01:02.0", Lang::Italiano), "segnalibro a 00:01:02.0");
        assert_eq!(rewind_hint(8.0, Lang::Italiano), "Riavvolgi a 8× — tieni premuto per scorrere, tocca per tornare indietro (F7)");
    }
}
