fn main() {
    let formulas = [
        ("The quadratic formula", r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}"),
        ("A Gaussian integral", r"\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}"),
        ("A matrix", r"A = \begin{pmatrix} a & b \\ c & d \end{pmatrix}"),
        ("A sum", r"\sum_{k=1}^{n} k = \frac{n(n+1)}{2}"),
        ("Maxwell, in full", r"\nabla \times \vec{B} - \frac{1}{c}\frac{\partial \vec{E}}{\partial t} = \frac{4\pi}{c}\vec{J}"),
        ("The limit that makes e", r"\lim_{n \to \infty} \left(1 + \frac{1}{n}\right)^n = e"),
    ];
    let mut page = String::from(
        "<!doctype html><meta charset=\"utf-8\"><style>body{font:15px/1.6 -apple-system,sans-serif;\
         margin:28px 34px;color:#1f2328}h2{font-size:15px;margin:18px 0 6px;color:#6b727d}\
         math{font-size:1.5em;display:block;margin:6px 0 18px}</style>",
    );
    for (title, tex) in formulas {
        if let Ok(html) = ownnotes_core::math::to_mathml(tex, true) {
            page.push_str(&format!("<h2>{title}</h2>{html}"));
        }
    }
    std::fs::write("/tmp/math-page.html", page).unwrap();
    println!("written");
}
