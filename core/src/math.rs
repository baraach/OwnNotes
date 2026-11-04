// LaTeX maths in notes. The formula is turned into MathML, which the web view
// draws on its own, so no script library or internet connection is needed.

use latex2mathml::{latex_to_mathml, DisplayStyle};

/// MathML for a LaTeX formula, inline or as its own centred line.
pub fn to_mathml(tex: &str, block: bool) -> Result<String, String> {
    let style = if block { DisplayStyle::Block } else { DisplayStyle::Inline };
    latex_to_mathml(tex.trim(), style).map_err(|e| format!("That formula couldn't be read: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas_become_mathml() {
        let out = to_mathml(r"\frac{a}{b} + x^2", false).unwrap();
        assert!(out.starts_with("<math"));
        assert!(out.contains("<mfrac>"));
        assert!(to_mathml(r"E = mc^2", true).unwrap().contains("display=\"block\""));
    }

    #[test]
    fn broken_formulas_are_an_error_not_a_panic() {
        assert!(to_mathml(r"\frac{a}{", false).is_err());
    }
}
