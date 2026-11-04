fn main() {
    let formulas = [
        ("quadratic", r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}"),
        ("gauss", r"\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}"),
        ("matrix", r"A = \begin{pmatrix} a & b \\ c & d \end{pmatrix}"),
        ("sum", r"\sum_{k=1}^{n} k = \frac{n(n+1)}{2}"),
        ("maxwell", r"\nabla \times \vec{B} - \frac{1}{c}\frac{\partial \vec{E}}{\partial t} = \frac{4\pi}{c}\vec{J}"),
        ("limit", r"\lim_{n \to \infty} \left(1 + \frac{1}{n}\right)^n = e"),
        ("fourier", r"f(x) = \frac{1}{\sqrt{2\pi}} \int_{-\infty}^{\infty} \hat{f}(\xi)\, e^{2\pi i \xi x}\, d\xi"),
    ];
    for (name, tex) in formulas {
        match ownnotes_core::math::to_mathml(tex, true) {
            Ok(html) => println!("OK   {name} ({} bytes)", html.len()),
            Err(e) => println!("FAIL {name}: {e}"),
        }
    }
}
