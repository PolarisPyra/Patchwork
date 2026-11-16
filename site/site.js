// Downloads, the mobile menu, the carousels and the pop-ins as you scroll. Everything here is progressive: without
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

  for (const carousel of document.querySelectorAll("[data-carousel]")) startCarousel(carousel);
}

// A carousel. A slide stays for as long as its pill takes to fill, and the fill only runs while the
// carousel is on screen and nobody has paused it, wherever the pointer is. The tabs beside the
// picture and the pills under it both pick a slide. With reduced motion it starts paused, on the
// first slide, and nothing plays until the visitor asks.
function startCarousel(root) {
  const slides = [...root.querySelectorAll("[data-slide]")];
  const pills = [...root.querySelectorAll("[data-pill]")];
  const tabs = [...root.querySelectorAll("[data-tab]")];
  const toggle = root.querySelector("[data-carousel-toggle]");
  let current = 0;
  let stopped = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  let onScreen = !("IntersectionObserver" in window);

  const sync = () => {
    const running = !stopped && onScreen;
    root.classList.toggle("is-running", running);
    root.classList.toggle("is-stopped", stopped);
    toggle?.setAttribute("aria-label", stopped ? "Play the slides" : "Pause the slides");
    const video = slides[current].querySelector("video");
    if (!video) return;
    if (running) video.play().catch(() => {});
    else video.pause();
  };

  const show = (index) => {
    slides[current].querySelector("video")?.pause();
    current = (index + slides.length) % slides.length;
    slides.forEach((slide, i) => slide.classList.toggle("is-active", i === current));
    pills.forEach((pill, i) => pill.classList.toggle("is-active", i === current));
    tabs.forEach((tab, i) => {
      tab.classList.toggle("is-active", i === current);
      if (i === current) tab.setAttribute("aria-current", "true");
      else tab.removeAttribute("aria-current");
    });
    const video = slides[current].querySelector("video");
    if (video) video.currentTime = 0;
    sync();
  };

  pills.forEach((pill, i) => {
    pill.addEventListener("click", () => show(i));
    // The spark's animations end on the pill too; only the fill moves the carousel on.
    pill.addEventListener("animationend", (event) => {
      if (event.animationName === "pill-fill") show(current + 1);
    });
  });
  tabs.forEach((tab, i) => tab.addEventListener("click", () => show(i)));
  tabs.forEach(ringTab);
  toggle?.addEventListener("click", () => {
    stopped = !stopped;
    sync();
  });

  if ("IntersectionObserver" in window) {
    new IntersectionObserver(
      ([entry]) => {
        onScreen = entry.isIntersecting;
        sync();
      },
      { threshold: 0.4 },
    ).observe(root);
  }
  sync();
}

function hostPlatform() {
  const platform = navigator.userAgentData?.platform ?? navigator.platform ?? "";
  const agent = navigator.userAgent;
  if (/mac/i.test(platform)) return "macos";
  if (/win/i.test(platform)) return "windows";
  if (/linux|x11/i.test(platform) && !/android/i.test(agent)) return "linux";
  return null;
}

// The ring around the active tab: two strokes from the middle of the left edge, one up and over the
// top, one down and under the bottom, each meeting the other in the middle of the right edge. Both
// are the same length, so they draw at the same steady pace. They're redrawn whenever the tab
// changes size. Their gradient spans the whole tab, so both halves match where they meet.
let ringCount = 0;
function ringTab(tab) {
  const ns = "http://www.w3.org/2000/svg";
  const id = `tab-ring-${ringCount++}`;
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("class", "tab-ring");
  svg.setAttribute("aria-hidden", "true");
  svg.innerHTML =
    `<defs><linearGradient id="${id}" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="100%" y2="0">` +
    `<stop offset="0" /><stop offset="0.5" /><stop offset="1" /></linearGradient></defs>` +
    `<path pathLength="1" stroke="url(#${id})" /><path pathLength="1" stroke="url(#${id})" />`;
  const [up, down] = svg.querySelectorAll("path");
  tab.append(svg);

  const fit = () => {
    const w = tab.clientWidth;
    const h = tab.clientHeight;
    const r = Math.min(parseFloat(getComputedStyle(tab).borderTopLeftRadius) || 0, h / 2) - 1;
    const mid = h / 2;
    up.setAttribute(
      "d",
      `M1 ${mid}V${1 + r}A${r} ${r} 0 0 1 ${1 + r} 1H${w - 1 - r}A${r} ${r} 0 0 1 ${w - 1} ${1 + r}V${mid}`,
    );
    down.setAttribute(
      "d",
      `M1 ${mid}V${h - 1 - r}A${r} ${r} 0 0 0 ${1 + r} ${h - 1}H${w - 1 - r}A${r} ${r} 0 0 0 ${w - 1} ${h - 1 - r}V${mid}`,
    );
  };
  fit();
  if ("ResizeObserver" in window) new ResizeObserver(fit).observe(tab);
}
