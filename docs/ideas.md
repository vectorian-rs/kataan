# Kataan Ideas

A lightweight parking lot for product and implementation ideas that are not yet committed to the main plan.

## Asset URI Markdown extension

Support an optional Kataan-specific asset URI in Markdown:

```md
![Diagram](asset:projects/foo/diagram.png)
```

Intent:

- Reference vault assets by canonical vault-relative path instead of fragile local relative paths.
- Let local editors keep using normal Markdown where possible, while Kataan can resolve `asset:` links in its own renderer and publishing pipeline.
- Publishing can upload the asset and rewrite the rendered/published output to a full HTTP URL without changing source Markdown.

Open questions:

- Should `asset:` be optional advanced syntax while `./diagram.png` remains the default?
- Should assets have TOML metadata, e.g. role, alt text, publish flag, checksum?
- Should publishing maintain a target-specific asset manifest mapping `asset:` URIs to CDN URLs?
