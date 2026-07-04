import * as CM from "@radix-ui/react-context-menu";

export const menuContentCls =
  "z-50 min-w-[190px] bg-card border border-line rounded-xl shadow-xl p-1.5 text-[12.5px]";

export function MItem(props: {
  icon: React.ReactNode;
  label: string;
  hint?: string;
  danger?: boolean;
  onClick: () => void;
}) {
  return (
    <CM.Item
      onSelect={props.onClick}
      className={`flex cursor-default items-center gap-2.5 rounded-lg px-2.5 py-1.5 outline-none data-[highlighted]:bg-side ${
        props.danger ? "text-danger" : ""
      }`}
    >
      {props.icon}
      <span className="flex-1">{props.label}</span>
      {props.hint && <span className="text-[10px] text-sub">{props.hint}</span>}
    </CM.Item>
  );
}

export const MSep = () => <CM.Separator className="my-1.5 h-px bg-line" />;
