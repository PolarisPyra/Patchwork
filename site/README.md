# Website

The product site: static pages, no build step, no dependencies. Everything it needs is in this
folder, so it can be dragged into Cloudflare Pages as it is.

```
site/
  index.html              the home page
  claude-code/ codex/ antigravity/ opencode/ agent-orchestrator/ ade/
                          one landing page each (index.html), for people searching for that
  styles.css              one stylesheet
  site.js                 platform detection for the download button, mobile menu, pop-ins on scroll
  refresh-downloads.mjs   points the page at the newest release tag
  _headers                Cloudflare Pages headers (CSP, cache lifetimes)
  robots.txt
  sitemap.xml             every page, for search engines
  fonts/                  Inter and JetBrains Mono, subset to Latin, self hosted
  img/                    screenshots of the app as WebP, plus the social card
  favicon.svg
  apple-touch-icon.png
```

## Landing pages

Each folder with an `index.html` is one page written by hand for one search, such as "Claude Code
GUI". They share `styles.css` and `site.js`, link to them from the root (`/styles.css`), and send
their download button to the home page's download section, so a release changes nothing in them.
Because of the root links, look at them through a local server rather than by opening the file:
`npx serve site`.

The header, the footer and its list of pages are repeated in every page. When one changes, change
it in all of them. A new page also goes in `sitemap.xml` and in each footer. Each page's FAQ is in
its `<head>` as well, as data for search engines; keep the two the same. What a page says about an
agent comes from `docs/agent-clis.md`.

## Deploy to Cloudflare Pages

One time, from the Cloudflare dashboard:

1. Workers & Pages → Create → Pages → **Upload assets**.
2. Name the project (for example `patchwork-site`), and upload the **contents of `site/`**, not the
   folder itself, so that `index.html` sits at the root.
3. Add the custom domain under the project's **Custom domains** tab. For `patchworkeditor.com`,
   point the apex at the Pages project. Cloudflare creates the record for you when the zone is on
   the same account.

The other way, from the command line, which redeploys in one command:

```sh
npx wrangler pages deploy site --project-name patchwork-site
```

Either way there is nothing to build: `site/` is already the deployable output. Uploading it does
not touch the releases, which live on GitHub.

## After a release

The page links into one GitHub release (`/releases/download/26.11.0/Patchwork_amd64.AppImage`,
and so on), whose files never change once published. The Release workflow points the page at the
new tag and commits the new links to main. The Site workflow (`.github/workflows/site.yml`) deploys
`site/` to the `patchwork-site` Pages project on every push to main that changes it, that commit
included, so nothing is done by hand. It needs two repository secrets, set once:

- `CLOUDFLARE_API_TOKEN`: a token with the **Account → Cloudflare Pages → Edit** permission.
- `CLOUDFLARE_ACCOUNT_ID`: the account the Pages project is on (`npx wrangler whoami` shows it).

Without the token the Site workflow fails and the page stays as it was. To refresh the links by hand:

```sh
node site/refresh-downloads.mjs          # rewrite index.html with the newest tag
node site/refresh-downloads.mjs --check  # exit 1 if it is stale, for CI
```

It reads `https://github.com/PolarisPyra/Patchwork/releases/latest/download/latest.json`, checks
that the release has all three files, and points the page's download links at its tag. A browser cannot do this itself: GitHub sends no CORS
headers for release files, so reading `latest.json` from the page would fail. That is why the tag
is written into the page at release time instead.

`site.js` only picks which of the three files the download button points at, from the visitor's
platform, and falls back to the download section on a phone or an unknown system.

## The social card

`img/og.png` is the preview chat apps and search results show for every page: the top of the home
page's hero at 1200 × 630. When the hero changes, take it again with any Chromium, with reduced
motion forced so nothing is still waiting to pop in, and cut off the header:

```sh
chromium --headless=new --hide-scrollbars --force-device-scale-factor=1 --force-prefers-reduced-motion \
  --window-size=1200,800 --virtual-time-budget=4000 --screenshot=hero.png "file://$PWD/site/index.html"
magick hero.png -crop 1200x630+0+100 +repage -strip site/img/og.png
```

Then raise the `?v=` on every page's `og:image` and `twitter:image`, and on the home page's
`"image"`, so chat apps that cache the card by its address fetch the new one.

## Before the domain changes

`index.html` names `https://patchworkeditor.com/` as its canonical URL and its social card base. If
the site is going somewhere else, change those, and the `og:image` path, to match.

The page names the release only in its four download links, all of which `refresh-downloads.mjs`
updates.
