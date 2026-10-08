import type { KeyboardEvent } from "react";

export function ChoiceTabs({
  label,
  value,
  choices,
  change,
  className = "",
  idPrefix,
}: {
  label: string;
  value: string;
  choices: { value: string; label: string }[];
  change: (value: string) => void;
  className?: string;
  idPrefix: string;
}) {
  function navigate(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    const offset =
      event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (!offset && event.key !== "Home" && event.key !== "End") return;
    event.preventDefault();
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? choices.length - 1
          : (index + offset + choices.length) % choices.length;
    change(choices[next].value);
    event.currentTarget.parentElement
      ?.querySelectorAll<HTMLButtonElement>('[role="tab"]')
      [next]?.focus();
  }
  return (
    <div
      className={`choice-tabs ${className}`}
      role="tablist"
      aria-label={label}
    >
      {choices.map((choice, index) => (
        <button
          key={choice.value}
          id={`${idPrefix}-${choice.value}`}
          role="tab"
          aria-selected={value === choice.value}
          aria-controls={`${idPrefix}-${choice.value}-panel`}
          tabIndex={value === choice.value ? 0 : -1}
          onClick={() => change(choice.value)}
          onKeyDown={(event) => navigate(event, index)}
        >
          {choice.label}
        </button>
      ))}
    </div>
  );
}
