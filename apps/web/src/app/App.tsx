import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { api, DEFAULT_SETTINGS, type Settings, type UpdateSettings } from "~/api";
import UpdateToast from "~/features/updates/UpdateToast";
import Workspace from "~/features/workspace/Workspace";

// Onboarding only runs on first launch, so it stays out of the main chunk.
const Onboarding = lazy(() => import("~/features/onboarding/Onboarding"));

function useResolvedTheme(theme: Settings["theme"]): "light" | "dark" {
  const query = "(prefers-color-scheme: dark)";
  const [systemDark, setSystemDark] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const mq = window.matchMedia(query);
    const onChange = () => setSystemDark(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);
  if (theme === "system") return systemDark ? "dark" : "light";
  return theme;
}

export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const resolvedTheme = useResolvedTheme(settings?.theme ?? DEFAULT_SETTINGS.theme);

  useEffect(() => {
    api.getSettings().then(setSettings, () => setSettings(DEFAULT_SETTINGS));
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = resolvedTheme;
  }, [resolvedTheme]);

  // Source of truth for sequential updates; React state updaters may run lazily.
  const settingsRef = useRef<Settings | null>(null);
  settingsRef.current = settings;

  const updateSettings = useCallback<UpdateSettings>(async (next) => {
    const previous = settingsRef.current;
    const resolved = typeof next === "function" ? next(previous ?? DEFAULT_SETTINGS) : next;
    settingsRef.current = resolved;
    setSettings(resolved);
    try {
      await api.saveSettings(resolved);
    } catch (e) {
      // A change that was not saved is taken back, unless a later one has replaced it already.
      if (settingsRef.current === resolved && previous) {
        settingsRef.current = previous;
        setSettings(previous);
      }
      throw e;
    }
  }, []);

  const toggleTheme = useCallback(() => {
    void updateSettings((s) => ({ ...s, theme: resolvedTheme === "dark" ? "light" : "dark" }));
  }, [resolvedTheme, updateSettings]);

  if (!settings) return null;

  return settings.onboarded ? (
    <>
      <Workspace settings={settings} updateSettings={updateSettings} toggleTheme={toggleTheme} />
      <UpdateToast />
    </>
  ) : (
    <Suspense fallback={null}>
      <Onboarding
        settings={settings}
        updateSettings={updateSettings}
        resolvedTheme={resolvedTheme}
        toggleTheme={toggleTheme}
      />
    </Suspense>
  );
}
