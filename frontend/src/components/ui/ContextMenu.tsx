import { useEffect, type ReactNode } from "react";

export interface ContextAction { label: string; disabled?: boolean; title?: string; danger?: boolean; onSelect: () => void }

export function ContextMenu({ x, y, title, actions, onClose, footer }: { x: number; y: number; title: string; actions: ContextAction[]; onClose: () => void; footer?: ReactNode }) {
  useEffect(() => {
    const close = () => onClose();
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    window.addEventListener("click", close);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    window.addEventListener("keydown", key);
    return () => { window.removeEventListener("click", close); window.removeEventListener("scroll", close, true); window.removeEventListener("resize", close); window.removeEventListener("keydown", key); };
  }, [onClose]);
  return <div className="woo-context-menu" role="menu" aria-label={title} style={{ left: Math.max(4, Math.min(x, window.innerWidth - 280)), top: Math.max(4, Math.min(y, window.innerHeight - Math.min(420, 40 + actions.length * 32))) }} onClick={(event) => event.stopPropagation()}>
    <p className="woo-context-title">{title}</p>
    {actions.map((action) => <button type="button" role="menuitem" key={action.label} disabled={action.disabled} title={action.title} className={action.danger ? "danger" : ""} onClick={() => { action.onSelect(); onClose(); }}>{action.label}</button>)}
    {footer}
  </div>;
}
