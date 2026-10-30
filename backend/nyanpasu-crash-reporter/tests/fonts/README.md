# CJK fallback fixtures

These generated TrueType fonts are dedicated to the public domain under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).
They contain an empty `.notdef` and one rectangular outline, with no third-party
font data. Both use 1000 units per em, ascent 800 and descent -200. The box spans
`(100, 0)` to `(700, 700)` with advance 800.

- `sparse.ttf` (Nyanpasu Test Sparse) maps only `告` to the box.
- `cjk.ttf` (Nyanpasu Test CJK) maps the report's Chinese regression characters
  `报告问题系统环境请在描述帮助我们修复诊断信息已就绪文件夹中显示复制关闭`
  to the box.

The rendering test replaces the Han fallback with the sparse font, then appends
the CJK font through the same helper as production. Every character must paint
ink. This reproduces incomplete script coverage without relying on installed
fonts, OS language settings or platform-specific reference images.
