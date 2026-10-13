//! Printers for Frameshift's output layer.
//!
//! This crate turns converted structure into new files: an Astro component
//! model with its canonical printer, the conversion of React JSX into that
//! model, and emitted text whose every byte maps to a source construct. It
//! performs no I/O, selects no migration rules, and executes nothing. Callers
//! decide which constructs to convert and classify the recorded assumptions.

mod astro;
mod html;
mod jsx;
mod names;
mod output;

pub use astro::{
    AstroComponent, AttributeItem, AttributeValue, Element, Expression, ExpressionPiece,
    FrontmatterItem, ImportItem, Indent, LineEnding, NamedImport, PrintError, PrintOptions,
    PropMember, PropType, TagName, TemplateNode, print_astro_component,
};
pub use jsx::{
    Assumption, AssumptionKind, ClientDirective, ComponentReference, ComponentStrategy,
    ConvertError, ConvertErrorKind, ConvertedMarkup, ElementResolver, JsxConversion, convert_jsx,
};
pub use names::{AttributeName, ComponentName, HtmlTagName, JsIdentifier};
pub use output::{
    Anchor, BuildError, EmitLimits, EmittedText, OutputBuilder, OutputRegion, ProvenanceError,
    RegionOrigin, SourceId,
};
