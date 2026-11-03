import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { CaretDownIcon } from "@phosphor-icons/react";
import type { AgentKind, ModelOption } from "~/api";
import { CHIP_CLASS } from "~/components/chip";
import ProviderLogos from "~/components/ProviderLogos";
import PickerPopup from "~/features/chat/composer/modelPicker/PickerPopup";

interface Props {
  agent: AgentKind;
  model: string | null;
  agents: AgentKind[];
  models: Partial<Record<AgentKind, ModelOption[]>>;
  onPick: (agent: AgentKind, model: string | null) => void;
  /** Opens the picker each time it changes, as a keyboard shortcut asks. */
  openRequest?: number;
}

/** The composer's model button, which opens the model picker. */
export default function ModelPicker(props: Props) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const handledOpenRequest = useRef(props.openRequest ?? 0);
  useEffect(() => {
    if (props.openRequest === handledOpenRequest.current) return;
    handledOpenRequest.current = props.openRequest ?? 0;
    if (props.openRequest) setOpen(true);
  }, [props.openRequest]);
  const label =
    props.models[props.agent]?.find((m) => m.id === props.model)?.label ?? props.model ?? "Default";
  const close = () => {
    setOpen(false);
    trigger.current?.focus({ preventScroll: true });
  };
  return (
    <>
      <button
        ref={trigger}
        className={CHIP_CLASS}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={`Choose model: ${label}`}
        onClick={() => setOpen(!open)}
      >
        <ProviderLogos kind={props.agent} model={props.model} size={16} />
        {/* A narrow composer shows only the model's logo; the button's name still says which. */}
        <span className="hidden @[480px]:inline">{label}</span>
        <CaretDownIcon size={14} />
      </button>
      {open &&
        createPortal(
          <PickerPopup {...props} anchor={trigger.current!} close={close} />,
          document.body,
        )}
    </>
  );
}
