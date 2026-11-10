import { LoaderCircle, X } from "lucide-react";
import {
  type ButtonHTMLAttributes,
  type HTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
  useEffect,
  useRef,
} from "react";
import { cn } from "@/lib/cn";

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: "sm" | "md" | "lg";
  icon?: ReactNode;
  loading?: boolean;
}

const buttonVariants: Record<ButtonVariant, string> = {
  primary: "gradient-fill text-white hover:brightness-110 border-transparent",
  secondary: "bg-raised text-ink border-line hover:border-faint/60",
  ghost: "bg-transparent text-soft border-transparent hover:bg-raised hover:text-ink",
  danger: "bg-bad text-white border-transparent hover:brightness-110",
};

const buttonSizes = {
  sm: "h-8 gap-1.5 px-3 text-[13px]",
  md: "h-9 gap-2 px-3.5 text-sm",
  lg: "h-11 gap-2 px-5 text-[15px]",
};

export function Button({
  variant = "secondary",
  size = "md",
  icon,
  loading,
  className,
  children,
  disabled,
  type = "button",
  ...props
}: ButtonProps) {
  return (
    <button
      type={type}
      disabled={disabled || loading}
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded-lg border font-medium whitespace-nowrap transition disabled:opacity-45",
        buttonVariants[variant],
        buttonSizes[size],
        className,
      )}
      {...props}
    >
      {loading ? <LoaderCircle className="size-4 animate-spin" aria-hidden /> : icon}
      {children}
    </button>
  );
}

export function Card({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn("rounded-2xl border border-line bg-panel/90 backdrop-blur-sm", className)}
      {...props}
    />
  );
}

export function Label({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("label", className)}>{children}</div>;
}

export function PageHeader({
  eyebrow,
  title,
  description,
  actions,
}: {
  eyebrow: string;
  title: string;
  description?: string;
  actions?: ReactNode;
}) {
  return (
    <header className="mb-5 flex items-end justify-between gap-6">
      <div className="min-w-0">
        <Label>{eyebrow}</Label>
        <h1 className="mt-1 text-[26px] leading-tight font-semibold tracking-tight">{title}</h1>
        {description && <p className="mt-1 max-w-2xl text-soft">{description}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </header>
  );
}

interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  description?: string;
  disabled?: boolean;
}

export function Toggle({ checked, onChange, label, description, disabled }: ToggleProps) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div className="min-w-0">
        <div className="text-sm font-medium">{label}</div>
        {description && <div className="mt-0.5 text-[13px] text-soft">{description}</div>}
      </div>
      <Switch checked={checked} onChange={onChange} label={label} disabled={disabled} />
    </div>
  );
}

export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative mt-0.5 h-[22px] w-[38px] shrink-0 rounded-full border transition disabled:opacity-45",
        checked ? "gradient-fill border-transparent" : "border-line bg-raised",
      )}
    >
      <span
        className={cn(
          "absolute top-px left-px size-[18px] rounded-full bg-white shadow-sm transition-transform",
          checked && "translate-x-4",
        )}
      />
    </button>
  );
}

export function Checkbox({ checked, label }: { checked: boolean; label: string }) {
  return (
    <span
      role="img"
      aria-label={`${label}: ${checked ? "selected" : "not selected"}`}
      className={cn(
        "flex size-[18px] shrink-0 items-center justify-center rounded-[5px] border transition",
        checked ? "gradient-fill border-transparent" : "border-faint/50 bg-transparent",
      )}
    >
      {checked && (
        <svg viewBox="0 0 12 12" className="size-3 text-white" fill="none" aria-hidden>
          <path
            d="M2.5 6.5 5 9l4.5-5.5"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      )}
    </span>
  );
}

export function Input({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={cn(
        "h-9 w-full min-w-0 rounded-lg border border-line bg-canvas/60 px-3 text-sm text-ink placeholder:text-faint",
        "focus:border-accent focus:outline-none",
        className,
      )}
      {...props}
    />
  );
}

export function Field({
  label,
  hint,
  error,
  children,
}: {
  label: string;
  hint?: string;
  error?: string | null;
  children: ReactNode;
}) {
  return (
    // biome-ignore lint/a11y/noLabelWithoutControl: the control is passed in as children.
    <label className="block">
      <span className="mb-1.5 block text-[13px] font-medium text-soft">{label}</span>
      {children}
      {error ? (
        <span className="mt-1 block text-[12px] text-bad">{error}</span>
      ) : (
        hint && <span className="mt-1 block text-[12px] text-faint">{hint}</span>
      )}
    </label>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <div role="radiogroup" aria-label={label} className="flex rounded-lg bg-raised p-0.5">
      {options.map((option) => (
        // biome-ignore lint/a11y/useSemanticElements: styled as a segmented button group.
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={value === option.value}
          onClick={() => onChange(option.value)}
          className={cn(
            "h-7 flex-1 rounded-md px-2.5 text-[13px] whitespace-nowrap transition",
            value === option.value
              ? "bg-panel font-medium text-ink shadow-sm"
              : "text-soft hover:text-ink",
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function Chip({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "h-7 rounded-full border px-3 text-[13px] transition",
        active
          ? "border-accent bg-accent/12 font-medium text-accent"
          : "border-line text-soft hover:text-ink",
      )}
    >
      {children}
    </button>
  );
}

type Tone = "neutral" | "accent" | "good" | "warn" | "bad";

const badgeTones: Record<Tone, string> = {
  neutral: "bg-raised text-soft",
  accent: "bg-accent/14 text-accent",
  good: "bg-good/14 text-good",
  warn: "bg-warn/16 text-warn",
  bad: "bg-bad/14 text-bad",
};

export function Badge({
  tone = "neutral",
  children,
  className,
}: {
  tone?: Tone;
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex h-[22px] items-center gap-1.5 rounded-md px-2 text-[12px] font-medium whitespace-nowrap",
        badgeTones[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}

const avatarColors = ["#3b82d6", "#7a63e0", "#0f9a82", "#c26a2d", "#c04d86", "#4c7a3f"];

function hashOf(text: string): number {
  let hash = 0;
  for (const character of text) hash = (hash * 31 + character.charCodeAt(0)) | 0;
  return Math.abs(hash);
}

export function Avatar({
  name,
  url,
  size = 32,
  square,
}: {
  name: string;
  url?: string | null;
  size?: number;
  square?: boolean;
}) {
  const radius = square ? "rounded-[28%]" : "rounded-full";
  if (url) {
    return (
      <img
        src={url}
        alt=""
        width={size}
        height={size}
        loading="lazy"
        className={cn("sensitive shrink-0 object-cover", radius)}
        style={{ width: size, height: size }}
      />
    );
  }
  const initial = Array.from(name.replace(/^[#@]/, "").trim())[0]?.toUpperCase() ?? "?";
  return (
    <span
      aria-hidden
      className={cn(
        "sensitive flex shrink-0 items-center justify-center font-medium text-white",
        radius,
      )}
      style={{
        width: size,
        height: size,
        fontSize: Math.round(size * 0.42),
        background: avatarColors[hashOf(name) % avatarColors.length],
      }}
    >
      {initial}
    </span>
  );
}

export function ProgressBar({ value, active }: { value: number; active?: boolean }) {
  return (
    <div
      role="progressbar"
      aria-valuenow={value}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-label="Progress"
      className="h-2 overflow-hidden rounded-full bg-raised"
    >
      <div
        className={cn(
          "gradient-fill relative h-full overflow-hidden rounded-full transition-[width] duration-300",
          active && "shimmer",
        )}
        style={{ width: `${Math.max(value, 1)}%` }}
      />
    </div>
  );
}

export function Stat({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="min-w-0" data-stat={label}>
      <div className="tabular truncate text-[26px] leading-tight font-semibold tracking-tight">
        {value}
      </div>
      <Label className="mt-1">{label}</Label>
      {hint && <div className="mt-0.5 text-[12px] text-faint">{hint}</div>}
    </div>
  );
}

export function Modal({
  title,
  onClose,
  children,
  footer,
  wide,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  wide?: boolean;
}) {
  const dialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const element = dialog.current;
    if (element && !element.open) element.showModal();
  }, []);

  return (
    <dialog
      ref={dialog}
      onClose={onClose}
      className={cn(
        "m-auto w-full rounded-2xl border border-line bg-panel p-0 text-ink backdrop:bg-black/55",
        wide ? "max-w-2xl" : "max-w-md",
      )}
    >
      <div className="flex items-center justify-between px-5 pt-4 pb-3">
        <h2 className="text-[16px] font-semibold">{title}</h2>
        <Button variant="ghost" size="sm" aria-label="Close" onClick={onClose} className="px-2">
          <X className="size-4" aria-hidden />
        </Button>
      </div>
      <div className="max-h-[62vh] overflow-y-auto px-5 pb-4">{children}</div>
      {footer && (
        <div className="flex justify-end gap-2 border-t border-line px-5 py-3">{footer}</div>
      )}
    </dialog>
  );
}

export function EmptyState({
  icon,
  title,
  children,
}: {
  icon: ReactNode;
  title: string;
  children?: ReactNode;
}) {
  return (
    <div className="flex h-full flex-col items-center justify-center px-8 py-14 text-center">
      <div className="mb-3 flex size-11 items-center justify-center rounded-xl bg-raised text-soft">
        {icon}
      </div>
      <div className="text-[15px] font-medium">{title}</div>
      {children && <div className="mt-1 max-w-md text-soft">{children}</div>}
    </div>
  );
}

export function Callout({
  tone = "warn",
  icon,
  children,
}: {
  tone?: "warn" | "bad" | "accent";
  icon: ReactNode;
  children: ReactNode;
}) {
  const tones = {
    warn: "border-warn/35 bg-warn/8 text-ink",
    bad: "border-bad/35 bg-bad/8 text-ink",
    accent: "border-accent/30 bg-accent/7 text-ink",
  };
  const iconTones = { warn: "text-warn", bad: "text-bad", accent: "text-accent" };
  return (
    <div className={cn("flex gap-2.5 rounded-xl border px-3.5 py-2.5 text-[13px]", tones[tone])}>
      <span className={cn("mt-px shrink-0 [&>svg]:size-4", iconTones[tone])}>{icon}</span>
      <div className="min-w-0">{children}</div>
    </div>
  );
}
