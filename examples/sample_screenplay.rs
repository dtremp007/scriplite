//! Generates `tests/fixtures/public/sample-screenplay.pdf`, an original
//! three-page screenplay used by the public end-to-end tests.
//!
//! cargo run --example sample_screenplay [-- OUTPUT.pdf]
//!
//! The layout follows standard US Letter screenplay margins in 12 pt Courier.
//! `tests/sample_screenplay.rs` checks that the committed PDF matches this output.

use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, Stream, dictionary};

const PAGE_WIDTH: f32 = 612.0;
const PAGE_HEIGHT: f32 = 792.0;
const FONT_SIZE: f32 = 12.0;
const ADVANCE: f32 = 7.2;
const LINE: f32 = 12.0;
const TOP: f32 = 720.0;
const BOTTOM: f32 = 72.0;

const ACTION_X: f32 = 108.0;
const ACTION_WIDTH: usize = 61;
const CUE_X: f32 = 266.4;
const PAREN_X: f32 = 223.2;
const PAREN_WIDTH: usize = 25;
const DIALOGUE_X: f32 = 180.0;
const DIALOGUE_WIDTH: usize = 35;
const RIGHT_MARGIN: f32 = 540.0;
const DUAL_LEFT_CUE_X: f32 = 162.0;
const DUAL_LEFT_X: f32 = 108.0;
const DUAL_RIGHT_CUE_X: f32 = 396.0;
const DUAL_RIGHT_X: f32 = 330.0;
const DUAL_WIDTH: usize = 20;

#[allow(dead_code)]
fn main() {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "tests/fixtures/public/sample-screenplay.pdf".to_string());
    std::fs::write(&output, build_pdf()).expect("write sample screenplay");
    println!("{output}");
}

enum Part<'a> {
    Paren(&'a str),
    Speech(&'a str),
}
use Part::{Paren, Speech};

#[derive(Default)]
struct Layout {
    pages: Vec<Vec<(f32, f32, String)>>,
    y: f32,
}

impl Layout {
    fn new_page(&mut self) {
        self.pages.push(Vec::new());
        self.y = TOP;
        let number = self.pages.len();
        if number > 1 {
            let label = format!("{number}.");
            let x = RIGHT_MARGIN - label.len() as f32 * ADVANCE;
            self.pages.last_mut().unwrap().push((x, 756.0, label));
        }
    }

    fn line(&mut self, x: f32, text: &str) {
        assert!(self.y >= BOTTOM, "page {} overflows", self.pages.len());
        let y = self.y;
        self.pages
            .last_mut()
            .unwrap()
            .push((x, y, text.to_string()));
        self.y -= LINE;
    }

    fn blank(&mut self) {
        self.y -= LINE;
    }

    fn wrapped(&mut self, x: f32, width: usize, text: &str) {
        for line in wrap(text, width) {
            self.line(x, &line);
        }
    }

    fn scene(&mut self, heading: &str) {
        self.line(ACTION_X, heading);
        self.blank();
    }

    fn action(&mut self, text: &str) {
        self.wrapped(ACTION_X, ACTION_WIDTH, text);
        self.blank();
    }

    fn transition(&mut self, text: &str) {
        self.line(RIGHT_MARGIN - text.len() as f32 * ADVANCE, text);
        self.blank();
    }

    /// A cue and its parts, without the trailing blank line.
    fn speech(&mut self, cue: &str, parts: &[Part]) {
        self.line(CUE_X, cue);
        for part in parts {
            match part {
                Paren(text) => self.wrapped(PAREN_X, PAREN_WIDTH, &format!("({text})")),
                Speech(text) => self.wrapped(DIALOGUE_X, DIALOGUE_WIDTH, text),
            }
        }
    }

    fn dialogue(&mut self, cue: &str, parts: &[Part]) {
        self.speech(cue, parts);
        self.blank();
    }

    fn dual(&mut self, left: (&str, &str), right: (&str, &str)) {
        assert!(self.y >= BOTTOM);
        let y = self.y;
        let page = self.pages.last_mut().unwrap();
        page.push((DUAL_LEFT_CUE_X, y, left.0.to_string()));
        page.push((DUAL_RIGHT_CUE_X, y, right.0.to_string()));
        self.y -= LINE;
        let left_lines = wrap(left.1, DUAL_WIDTH);
        let right_lines = wrap(right.1, DUAL_WIDTH);
        for index in 0..left_lines.len().max(right_lines.len()) {
            assert!(self.y >= BOTTOM);
            let y = self.y;
            let page = self.pages.last_mut().unwrap();
            if let Some(line) = left_lines.get(index) {
                page.push((DUAL_LEFT_X, y, line.clone()));
            }
            if let Some(line) = right_lines.get(index) {
                page.push((DUAL_RIGHT_X, y, line.clone()));
            }
            self.y -= LINE;
        }
        self.blank();
    }
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn screenplay() -> Layout {
    let mut s = Layout::default();

    s.new_page();
    s.line(ACTION_X, "FADE IN:");
    s.blank();
    s.scene("INT. NIGHT TRAIN - DINING CAR - NIGHT");
    s.action(
        "Rain streaks the black windows. Empty tables rattle with the \
         rhythm of the rails. MARA (30s, coat still buttoned) stirs a \
         coffee that went cold an hour ago.",
    );
    s.action("DEV (30s, soaked, out of breath) drops into the seat across from her.");
    s.dialogue("DEV", &[Speech("You kept the ticket.")]);
    s.dialogue(
        "MARA",
        &[
            Paren("not looking up"),
            Speech("I keep everything. That has always been the problem."),
        ],
    );
    s.dialogue("DEV", &[Speech("Then keep this too.")]);
    s.action("He slides a small brass key across the table. She stops stirring.");
    s.dialogue("MARA", &[Speech("Where did you find it?")]);
    s.speech(
        "DEV",
        &[Speech(
            "Exactly where you left it. Taped under the third step of the \
             old station house, next to the timetable nobody ever read. I \
             went back every winter for six years, because you said the \
             lock would only open on the night the last train ran, and I \
             believed you, the way I believed everything you told me back \
             then.",
        )],
    );
    s.line(CUE_X, "(MORE)");

    s.new_page();
    s.dialogue(
        "DEV (CONT'D)",
        &[Speech("Tonight is that night. So here I am.")],
    );
    s.action("The overhead lights flicker. Both of them start talking at once.");
    s.dual(
        ("MARA", "You were never supposed to come back for it."),
        ("DEV", "I was never supposed to let you leave alone."),
    );
    s.action("Silence. The train slows. Brakes hiss somewhere beneath them.");
    s.dialogue(
        "CONDUCTOR (V.O.)",
        &[Speech(
            "Next stop, Harrow Junction. Harrow Junction, end of the line.",
        )],
    );
    s.dialogue(
        "MARA",
        &[
            Paren("quietly"),
            Speech("End of the line. Of course it is."),
        ],
    );
    s.transition("CUT TO:");

    s.new_page();
    s.scene("EXT. HARROW JUNCTION - PLATFORM - DAWN");
    s.action(
        "Fog over empty tracks. Mara and Dev step down from the last car \
         as the train pulls away behind them. A rusted lockbox hangs on \
         the station wall.",
    );
    s.dialogue("DEV", &[Speech("Your hands are shaking.")]);
    s.dialogue(
        "MARA",
        &[
            Speech("It's the cold."),
            Paren("beat"),
            Speech("It isn't the cold."),
        ],
    );
    s.action("She fits the brass key into the lock. It turns.");
    s.transition("FADE OUT.");
    s
}

pub fn build_pdf() -> Vec<u8> {
    let layout = screenplay();
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Courier",
        "Encoding" => "WinAnsiEncoding",
    });
    let resources_id = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let mut kids = Vec::new();
    for lines in &layout.pages {
        let mut operations = vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), FONT_SIZE.into()]),
        ];
        for (x, y, text) in lines {
            operations.push(Operation::new(
                "Tm",
                vec![
                    1.into(),
                    0.into(),
                    0.into(),
                    1.into(),
                    (*x).into(),
                    (*y).into(),
                ],
            ));
            operations.push(Operation::new(
                "Tj",
                vec![Object::string_literal(text.as_str())],
            ));
        }
        operations.push(Operation::new("ET", vec![]));
        let content = Content { operations }.encode().expect("encode content");
        let content_id = document.add_object(Stream::new(dictionary! {}, content));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
        });
        kids.push(page_id.into());
    }
    let page_count = kids.len() as i64;
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => page_count,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), PAGE_WIDTH.into(), PAGE_HEIGHT.into()],
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    let info_id = document.add_object(dictionary! {
        "Title" => Object::string_literal("Sample Screenplay"),
        "Producer" => Object::string_literal("scriplite examples/sample_screenplay.rs"),
    });
    document.trailer.set("Root", catalog_id);
    document.trailer.set("Info", info_id);
    document.compress();
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("serialize PDF");
    bytes
}
