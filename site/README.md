# Website

The product site: one static page, no build step, no dependencies. Everything it needs is in this
folder, so it can be dragged into Cloudflare Pages as it is.

```
site/
  index.html              the whole page
  styles.css              one stylesheet
  site.js                 platform detection for the download button, mobile menu, promo video
  refresh-downloads.mjs   points the page at the newest release tag
  _headers                Cloudflare Pages headers (CSP, cache lifetimes)
  robots.txt
  fonts/                  Inter and JetBrains Mono, subset to Latin, self hosted
  img/                    the promo poster as WebP, plus the social card
  video/                  the promo loop in the hero, as WebM (VP9) and MP4 (H.264)
  favicon.svg
  apple-touch-icon.png
```

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
not touch the update server (`updates.patchworkeditor.com`), which is a separate Bunny pull zone on a
DNS-only record.

## After a release

The page links into the per-tag folder on the update server (`/v26.11.0/Patchwork_amd64.AppImage`,
and so on), which never changes once published. The Release workflow's `site` job points the page
at the new tag, deploys it to the `patchwork-site` Pages project, and commits the new links to main,
so nothing is done by hand. It needs two repository secrets, set once:

- `CLOUDFLARE_API_TOKEN`: a token with the **Account → Cloudflare Pages → Edit** permission.
- `CLOUDFLARE_ACCOUNT_ID`: the account the Pages project is on (`npx wrangler whoami` shows it).

Without the token the job still commits the links but does not deploy. To do it by hand:

```sh
node site/refresh-downloads.mjs          # rewrite index.html with the newest tag
node site/refresh-downloads.mjs --check  # exit 1 if it is stale, for CI
```

It reads `https://updates.patchworkeditor.com/latest.json`, checks that the release has all three
files, and updates the download links, the release line in the hero and the version in the
download section. A browser cannot do this itself: the update server sends no CORS headers, so
reading `latest.json` from the page would fail. That is why the tag is written into the page at
release time instead.

`site.js` only picks which of the three files the download button points at, from the visitor's
platform, and falls back to the download section on a phone or an unknown system.

## Before the domain changes

`index.html` names `https://patchworkeditor.com/` as its canonical URL and its social card base. If
the site is going somewhere else, change those, and the `og:image` path, to match.

The page describes version 26.11.0 in three places, all of which `refresh-downloads.mjs` updates.
