import type { ParamSpec } from "../types";

interface Props {
  spec: ParamSpec;
  value: unknown;
  onChange(name: string, value: unknown): void;
}

/** 把任意已存值转为受控输入框可用的字符串。 */
function toInputValue(v: unknown): string {
  if (v === null || v === undefined) return "";
  if (typeof v === "string") return v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  return JSON.stringify(v);
}

/** 按 `ParamSpec.kind` 把输入框的原始字符串转为要提交的类型化值。 */
function coerce(spec: ParamSpec, raw: string): unknown {
  switch (spec.kind) {
    case "Bool":
      return raw === "true";
    case "Integer":
      if (raw.trim() === "") return "";
      return /^[+-]?\d+$/.test(raw.trim()) ? Number(raw) : raw;
    case "Number": {
      if (raw.trim() === "") return "";
      const n = Number(raw);
      return Number.isFinite(n) ? n : raw;
    }
    case "List": {
      try {
        const v: unknown = JSON.parse(raw);
        return Array.isArray(v) ? v : raw;
      } catch {
        return raw;
      }
    }
    default:
      return raw;
  }
}

const KIND_LABEL: Record<ParamSpec["kind"], string> = {
  Number: "Number",
  Integer: "Integer",
  String: "String",
  Bool: "Bool",
  List: "List",
  Choice: "Choice",
  Any: "Any",
};

export default function VarField({ spec, value, onChange }: Props) {
  const required = spec.required;
  const desc = spec.desc || spec.description;

  const control = () => {
    if (spec.kind === "Bool") {
      return (
        <select
          value={value === true ? "true" : "false"}
          onChange={(e) => onChange(spec.name, coerce(spec, e.target.value))}
        >
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      );
    }
    if (spec.kind === "Choice" && spec.options && spec.options.length > 0) {
      return (
        <select
          value={toInputValue(value)}
          onChange={(e) => onChange(spec.name, coerce(spec, e.target.value))}
        >
          {!required && <option value="">（未选）</option>}
          {spec.options.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
      );
    }
    const numeric = spec.kind === "Number" || spec.kind === "Integer";
    return (
      <input
        type={numeric ? "number" : "text"}
        value={toInputValue(value)}
        step={spec.kind === "Integer" ? 1 : undefined}
        min={spec.min ?? undefined}
        max={spec.max ?? undefined}
        placeholder={spec.kind === "List" ? '例如 ["a","b"]' : undefined}
        onChange={(e) => onChange(spec.name, coerce(spec, e.target.value))}
      />
    );
  };

  const range =
    spec.min !== null || spec.max !== null
      ? `范围 ${spec.min ?? "-∞"} ~ ${spec.max ?? "+∞"}${spec.unit ? " " + spec.unit : ""}`
      : spec.unit
        ? `单位 ${spec.unit}`
        : "";

  return (
    <div className="var-field">
      <div className="var-head">
        <span className="var-name">{spec.name}</span>
        <span className={`var-type ${required ? "req" : "opt"}`}>
          {KIND_LABEL[spec.kind]} · {required ? "必填" : "可选"}
        </span>
      </div>
      {control()}
      {(desc || range) && (
        <div className="var-desc">
          {desc}
          {desc && range ? " · " : ""}
          {range}
        </div>
      )}
    </div>
  );
}
