// Downloads, the mobile menu and the promo video. Everything here is progressive: without JavaScript
// the links are real anchors and the page is complete.

const downloads = new Map();
for (const link of document.querySelectorAll("[data-download]")) {
  downloads.set(link.dataset.download, link.getAttribute("href"));
}

const nameFor = { linux: "Linux", windows: "Windows", macos: "macOS" };

// The footer's year is written into the page too, for a visitor without JavaScript.
for (const year of document.querySelectorAll("[data-year]")) {
  year.textContent = String(new Date().getFullYear());
}

function hostPlatform() {
  const platform = navigator.userAgentData?.platform ?? navigator.platform ?? "";
  const agent = navigator.userAgent;
  if (/mac/i.test(platform)) return "macos";
  if (/win/i.test(platform)) return "windows";
  if (/linux|x11/i.test(platform) && !/android/i.test(agent)) return "linux";
  return null;
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

// The promo video plays while it is on screen, and waits for a click for a visitor who asked for
// less motion. Without JavaScript it keeps the browser's own controls.
const promo = document.querySelector("[data-promo]");
const promoBar = promo?.querySelector("[data-promo-bar]");
const promoToggle = promo?.querySelector("[data-promo-toggle]");
const promoSeek = promo?.querySelector("[data-promo-seek]");
const promoTime = promo?.querySelector("[data-promo-time]");

if (promo && promoBar && promoToggle && promoSeek && promoTime) {
  const video = promo.querySelector("video");
  let stopped = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  let onScreen = false;

  const sync = () => {
    promo.classList.toggle("is-paused", video.paused);
    promoToggle.setAttribute("aria-label", video.paused ? "Play video" : "Pause video");
  };
  const update = () => {
    if (onScreen && !stopped) video.play().catch(() => {});
    else video.pause();
  };

  // The scrubber follows the video every frame while it plays, so its fill moves smoothly.
  const clock = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
  const showTime = () => {
    const total = Number(promoSeek.max);
    const now = Math.min(video.currentTime, total);
    promoSeek.value = String(now);
    promoSeek.style.setProperty("--p", `${(now / total) * 100}%`);
    promoSeek.setAttribute("aria-valuetext", `${clock(now)} of ${clock(total)}`);
    promoTime.textContent = `${clock(now)} / ${clock(total)}`;
  };
  const follow = () => {
    showTime();
    if (!video.paused) requestAnimationFrame(follow);
  };

  video.removeAttribute("controls");
  promoBar.hidden = false;
  video.addEventListener("loadedmetadata", () => {
    if (Number.isFinite(video.duration)) promoSeek.max = String(video.duration);
    showTime();
  });
  video.addEventListener("play", () => requestAnimationFrame(follow));
  video.addEventListener("seeked", showTime);
  promoSeek.addEventListener("input", () => {
    video.currentTime = Number(promoSeek.value);
    showTime();
  });
  video.addEventListener("play", sync);
  video.addEventListener("pause", sync);
  promoToggle.addEventListener("click", () => {
    stopped = !video.paused;
    if (stopped) video.pause();
    else video.play().catch(() => {});
  });

  new IntersectionObserver(([entry]) => {
    onScreen = entry.isIntersecting;
    update();
  }, { threshold: 0.25 }).observe(video);
  sync();
  showTime();
}
