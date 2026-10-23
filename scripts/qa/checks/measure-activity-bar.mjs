// Reports how far each tool-stripe icon sits from the center of its button and of the stripe.
import { openWorkspace } from "../lib/open.mjs";

const { browser, page } = await openWorkspace();
const boxes = await page.locator('button[aria-label="Tasks"]').locator("..").evaluate((nav) => {
  const r = (e) => {
    const b = e.getBoundingClientRect();
    return { cx: b.x + b.width / 2, w: b.width };
  };
  const cs = getComputedStyle(nav);
  const borderRight = parseFloat(cs.borderRightWidth) || 0;
  return {
    stripeCenter: (nav.getBoundingClientRect().x + (nav.getBoundingClientRect().width - borderRight) / 2),
    buttons: [...nav.querySelectorAll("button")].map((b) => ({
      label: b.getAttribute("aria-label"),
      buttonCenter: r(b).cx,
      iconCenter: r(b.querySelector("svg")).cx,
    })),
  };
});
await browser.close();

console.log(`stripe center: ${boxes.stripeCenter.toFixed(2)}px`);
for (const b of boxes.buttons) {
  const off = (b.iconCenter - b.buttonCenter).toFixed(2);
  console.log(`${b.label.padEnd(10)} button ${b.buttonCenter.toFixed(2)}  icon ${b.iconCenter.toFixed(2)}  icon offset ${off}px`);
}
