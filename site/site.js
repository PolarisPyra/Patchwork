// Downloads and the mobile menu. Everything here is progressive: without JavaScript the links are
// real anchors and the page is complete.

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
