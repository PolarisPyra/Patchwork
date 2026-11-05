// Downloads, the mobile menu and the pop-ins as you scroll. Everything here is progressive: without
// JavaScript the links are real anchors and the page is complete.
//
// It runs in the head, so the page knows it can hold blocks back for their pop-in before anything
// is painted, and does the rest once the page is parsed.
document.documentElement.classList.add("js");
document.addEventListener("DOMContentLoaded", start);

function start() {
  const downloads = new Map();
  for (const link of document.querySelectorAll("[data-download]")) {
    downloads.set(link.dataset.download, link.getAttribute("href"));
  }

  const nameFor = { linux: "Linux", windows: "Windows", macos: "macOS" };

  // The footer's year is written into the page too, for a visitor without JavaScript.
  for (const year of document.querySelectorAll("[data-year]")) {
    year.textContent = String(new Date().getFullYear());
  }

  const platform = hostPlatform();
  const primary = document.querySelector("[data-download-label]");

  if (primary) {
    if (platform && downloads.has(platform)) {
      primary.textContent = `Download for ${nameFor[platform]}`;
      primary.setAttribute("href", downloads.get(platform));
    } else {
      // A phone or an unknown system: send the visitor to the three downloads instead of guessing.
      primary.textContent = "Choose your download";
      primary.setAttribute("href", "#download");
    }
  }

  // The visitor's own system's card is the one in pink.
  const yours = platform && document.querySelector(`.dl [data-download="${platform}"]`);
  yours?.closest(".dl")?.classList.add("is-yours");

  // Only the page under the header scrolls. Before anything has focus, the scroll keys reach the
  // document, which no longer scrolls, so the first one hands focus to the page and scrolls it.
  const page = document.querySelector("[data-page]");
  const scrollKeys = new Set([" ", "PageDown", "PageUp", "Home", "End", "ArrowDown", "ArrowUp"]);

  if (page) {
    document.addEventListener("keydown", (event) => {
      if (scrollKeys.has(event.key) && document.activeElement === document.body) {
        page.focus({ preventScroll: true });
      }
    });
  }

  const nav = document.querySelector(".nav");
  const toggle = document.querySelector("[data-nav-toggle]");

  if (nav && toggle) {
    const setOpen = (open) => {
      nav.classList.toggle("is-open", open);
      toggle.setAttribute("aria-expanded", String(open));
      toggle.setAttribute("aria-label", open ? "Close menu" : "Open menu");
    };

    toggle.addEventListener("click", () => setOpen(!nav.classList.contains("is-open")));

    for (const link of nav.querySelectorAll(".nav-links a")) {
      link.addEventListener("click", () => setOpen(false));
    }

    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && nav.classList.contains("is-open")) {
        setOpen(false);
        toggle.focus();
      }
    });

    const wide = window.matchMedia("(min-width: 861px)");
    wide.addEventListener("change", (event) => {
      if (event.matches) setOpen(false);
    });
  }

  // Each block pops in once, the first time it scrolls into view. The styles only hold blocks back
  // when motion is welcome, so with reduced motion they are simply there.
  const blocks = document.querySelectorAll(".reveal");
  if ("IntersectionObserver" in window) {
    const seen = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          entry.target.classList.add("is-seen");
          seen.unobserve(entry.target);
        }
      },
      { root: page, threshold: 0.15 },
    );
    for (const block of blocks) seen.observe(block);
  } else {
    for (const block of blocks) block.classList.add("is-seen");
  }
}

function hostPlatform() {
  const platform = navigator.userAgentData?.platform ?? navigator.platform ?? "";
  const agent = navigator.userAgent;
  if (/mac/i.test(platform)) return "macos";
  if (/win/i.test(platform)) return "windows";
  if (/linux|x11/i.test(platform) && !/android/i.test(agent)) return "linux";
  return null;
}
