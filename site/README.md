# Official website

The site at <https://brylie.github.io/LoomDSP/> is plain HTML and CSS. There is no build step, so anyone can edit it.

- `index.html` is the page and `style.css` is its style.
- Screenshots live in `docs/images/`. The workflow in `.github/workflows/pages.yml` copies them into the site when it deploys, so the repository keeps one copy.
- Keep the writing style of the rest of the project: short, plain sentences, no em dashes. Write for musicians and sound designers, not developers. Do not name other products' modules.

## Preview locally

```sh
mkdir -p site/images && cp docs/images/*.png site/images/   # git ignores site/images
python3 -m http.server -d site 8000
```

Then open <http://localhost:8000>.

## Deploy

Every push to `main` that changes `site/` or `docs/images/` deploys the site. One-time setup: in the GitHub repository, open Settings, Pages, and set Source to GitHub Actions.
