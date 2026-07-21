# EvalCore documentation site

The source for the EvalCore documentation site and landing page, built with
[Astro Starlight](https://starlight.astro.build). It is deployed to GitHub Pages
by `.github/workflows/docs.yml` on pushes to `main` that touch `site/**`.

## Develop

```sh
npm install       # install dependencies
npm run dev       # local dev server at http://localhost:4321
npm run build     # production build to ./dist/
npm run preview   # preview the production build locally
```

Content lives in `src/content/docs/`, where each `.md`/`.mdx` file is a route
based on its path. The landing page is `src/content/docs/index.mdx` (Starlight `splash`
template).

## Base path

`astro.config.mjs` sets `site: https://evalcore.cc` and `base: /`, so the site serves
from the custom domain at the root. `public/CNAME` pins that domain across GitHub Pages
deploys. Internal content links are root-relative (no path prefix).

## Demo tape

`demo/quickstart.tape` is the [charmbracelet VHS](https://github.com/charmbracelet/vhs)
source for the terminal demo (record run, then instant `$0` replay). VHS is not
part of the build; render the GIF locally and commit it:

```sh
vhs demo/quickstart.tape
```
