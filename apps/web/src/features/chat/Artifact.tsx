import { memo, useEffect, useMemo, useRef, useState } from "react";
import { CodeIcon, EyeIcon } from "@phosphor-icons/react";
import { ACTION_CLASS, CopyButton } from "~/features/chat/markdown/CopyButton";

/**
 * Rendered artifacts from a reply, the way Claude desktop and Codex show them: a Mermaid diagram
 * drawn in place, and an HTML or SVG artifact run in a sandboxed frame. Every provider's replies
 * go through the same Markdown renderer, so each of them gets this.
 */

/** Languages rendered as a diagram. */
const DIAGRAM = "mermaid";
/** Languages rendered as a runnable artifact. */
const ARTIFACTS = new Set(["html", "htm", "svg", "xhtml"]);

/** Whether a fence's language is drawn rather than shown as code. */
export function isArtifact(lang: string): boolean {
  const name = lang.trim().toLowerCase();
  return name === DIAGRAM || ARTIFACTS.has(name);
}

/** Quiet time before a streaming artifact is re-rendered. */
const RENDER_DELAY_MS = 300;
/** Tallest a diagram or artifact shows before it scrolls. */
const FRAME_HEIGHT = 360;

type MermaidApi = {
  initialize: (config: Record<string, unknown>) => void;
  render: (id: string, code: string) => Promise<{ svg: string }>;
};

let mermaidPromise: Promise<MermaidApi> | null = null;
/** Mermaid is 25 MB of parser, so it loads the first time a reply has a diagram. */
function mermaid(): Promise<MermaidApi> {
  mermaidPromise ??= import("mermaid").then((module) => {
    const api = module.default as unknown as MermaidApi;
    api.initialize({
      startOnLoad: false,
      // An artifact comes from a model, so it may not load anything or run scripts in the page.
      securityLevel: "strict",
      theme: dark() ? "dark" : "default",
      fontFamily: "var(--font-sans)",
    });
    return api;
  });
  return mermaidPromise;
}

function dark(): boolean {
  return document.documentElement.dataset.theme !== "light";
}

let diagramCount = 0;

/** A Mermaid diagram, drawn from its source. */
const Mermaid = memo(function Mermaid({ code }: { code: string }) {
  const [svg, setSvg] = useState<{ code: string; html: string } | null>(null);
  const [error, setError] = useState<{ code: string; message: string } | null>(null);
  const upToDate = svg?.code === code;
  useEffect(() => {
    if (upToDate) return;
    let cancelled = false;
    const timer = setTimeout(() => {
      mermaid()
        .then((api) => api.render(`patchwork-diagram-${++diagramCount}`, code))
        .then(({ svg }) => !cancelled && (setSvg({ code, html: svg }), setError(null)))
        .catch((e: unknown) => {
          if (cancelled) return;
          const message = e instanceof Error ? e.message : String(e);
          setError({ code, message: message.split("\n")[0] });
        });
    }, RENDER_DELAY_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [code, upToDate]);
  if (error?.code === code) {
    return (
      <div className="px-3 py-2.5 text-[length:var(--fs-sm)] text-fg-muted">
        This diagram could not be drawn: {error.message}
      </div>
    );
  }
  if (!upToDate) {
    return <div className="px-3 py-2.5 text-[length:var(--fs-sm)] text-fg-subtle">Drawing…</div>;
  }
  // Mermaid rendered with securityLevel "strict", which refuses HTML and links in labels.
  return (
    <div
      className="flex justify-center overflow-auto px-3 py-3 [&_svg]:max-w-full"
      dangerouslySetInnerHTML={{ __html: svg.html }}
    />
  );
});

/** An HTML or SVG artifact, run in a frame that cannot reach the app. */
const ArtifactFrame = memo(function ArtifactFrame({ code, lang }: { code: string; lang: string }) {
  const [height, setHeight] = useState(FRAME_HEIGHT);
  const [empty, setEmpty] = useState(false);
  const frame = useRef<HTMLIFrameElement>(null);
  const doc = useMemo(() => {
    if (lang === "svg" && !/^\s*<(?:svg|!doctype|html)/i.test(code)) {
      return `<body style="margin:0;display:grid;place-items:center;min-height:100vh">${code}</body>`;
    }
    return code;
  }, [code, lang]);
  // The frame reports its own height, so a short artifact does not leave a tall empty box.
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      if (event.source !== frame.current?.contentWindow) return;
      const data = event.data as { type?: string; height?: number; empty?: boolean } | null;
      if (data?.type === "patchwork-artifact-height" && typeof data.height === "number") {
        setHeight(Math.min(Math.max(data.height + 8, 80), 900));
        setEmpty(Boolean(data.empty));
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);
  // A streaming artifact is rebuilt as it grows; clear the note until the frame reports again, so a
  // partial render does not flash "nothing visible".
  useEffect(() => setEmpty(false), [code]);
  // A frame with no same-origin access cannot read the page it runs, so the height comes from a
  // snippet added to the document. It measures the body, not the document element, whose height is
  // never less than the frame's own and would keep the frame at the height it started with. The same
  // snippet reports whether the body rendered anything visible, so an artifact that draws nothing — a
  // bare broken image, or a script that threw before it touched the DOM — shows a note, not an empty box.
  const measured = `<script>(function(){function v(){var b=document.body;if(!b)return false;if(b.innerText&&b.innerText.trim())return true;var m=b.querySelectorAll('img,svg,canvas,video,picture,object,embed,iframe');for(var i=0;i<m.length;i++){var e=m[i];if(e.tagName==='IMG'){if(e.complete&&e.naturalWidth>0)return true;}else return true;}return false;}function p(){parent.postMessage({type:"patchwork-artifact-height",height:document.body.scrollHeight,empty:!v()},"*");}new ResizeObserver(p).observe(document.body);setTimeout(p,200);})()</script>`;
  const srcDoc = /<\/body>/i.test(doc)
    ? doc.replace(/<\/body>/i, `${measured}</body>`)
    : `${doc}${measured}`;
  return (
    <div className="relative">
      <iframe
        ref={frame}
        title="Artifact"
        sandbox="allow-scripts"
        srcDoc={srcDoc}
        className="w-full border-0 bg-white"
        style={{ height: empty ? 80 : height }}
      />
      {empty && (
        <div className="absolute inset-0 grid place-items-center bg-panel px-3 text-center text-[length:var(--fs-sm)] text-fg-subtle">
          This artifact rendered nothing visible.
        </div>
      )}
    </div>
  );
});

/** A diagram or artifact block, with a toggle back to its source. */
export default function Artifact({
  lang,
  code,
  title,
}: {
  lang: string;
  code: string;
  title: string | null;
}) {
  const name = lang.trim().toLowerCase();
  const [showCode, setShowCode] = useState(false);
  const diagram = name === DIAGRAM;
  return (
    <div
      className="my-2 overflow-hidden rounded-[var(--radius)] border border-border bg-panel"
      data-artifact={name}
      data-artifact-source={code}
    >
      <div
        data-copy-skip
        className="flex items-center justify-between border-b border-border py-0.5 pr-1 pl-2.5 text-[length:var(--fs-xs)] text-fg-subtle"
      >
        <span className="min-w-0 truncate">{title || (diagram ? "Diagram" : "Artifact")}</span>
        <div className="flex items-center">
          <button
            className={`${ACTION_CLASS} aria-pressed:text-fg`}
            aria-pressed={showCode}
            onClick={() => setShowCode(!showCode)}
            title={showCode ? "Show the artifact" : "Show its source"}
            aria-label="Show source"
          >
            {showCode ? <EyeIcon size={13} /> : <CodeIcon size={13} />}
          </button>
          <CopyButton text={code} />
        </div>
      </div>
      {showCode ? (
        <pre className="m-0 overflow-x-auto px-3 py-2.5 text-[12px] leading-[1.5]">
          <code>{code}</code>
        </pre>
      ) : diagram ? (
        <Mermaid code={code} />
      ) : (
        <ArtifactFrame code={code} lang={name} />
      )}
    </div>
  );
}
