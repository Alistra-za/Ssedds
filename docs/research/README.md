# Research

The research behind Tessalith's defaults, in these pages. Each page says what was asked, how it was measured, what came out and what Tessalith does because of it. For a one-page summary, see [Research behind the defaults](../wiki/Research.md). Individual decisions are recorded in [DECISIONS.md](../../.claude/DECISIONS.md).

| Page | What it covers |
|---|---|
| [How Tessalith came about](design-overview.md) | The design: why Skyrim policy comes before encoder choice, Skyrim's texture rules, the targets, the Rust workspace, the pipeline, and how each open question was settled. |
| [BCn encoders: published evidence](encoder-survey.md) | A literature review of BC1 to BC7 encoders: published benchmarks, how BC7 works, alpha, perceptual weighting, RDO and maintenance status. |
| [Local encoder benchmark](encoder-benchmark.md) | Three rounds of measurements (up to 100 images per set, with confidence intervals) for BC1, BC3, BC4, BC5, BC6H and BC7, plus the BC4/BC5 search-radius test. |
| [Mipmaps, normal maps and alpha coverage](mipmaps-and-alpha.md) | Mip filters, linear-light filtering, normal renormalisation and the roughness bake, data maps, and coverage-preserving alpha mips. |
| [bc7e tuning](bc7e-tuning.md) | Whether bc7e's settings above `veryslow` give more BC7 quality, and what `max` costs. |
| [SSIM and VIF on the encoder benchmark](encoder-ssim-vif.md) | The benchmark's encodes rescored with Gaussian SSIM and multi-scale VIF. |
| [BC3 against BC7 alpha weights](bc7-alpha-weights.md) | Whether BC7 with a higher alpha weight can replace BC3 for delicate alpha, per texture class. |
