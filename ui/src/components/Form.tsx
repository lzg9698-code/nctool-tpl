import { useState } from "react";
import { isLosslessNumber } from "lossless-json";
import { Plus, Trash2, ChevronDown } from "lucide-react";
import {
  DraftNumber,
  numberText,
  typeOf,
  defaultValue,
  encode,
  type Schema,
  type Data,
} from "../lib/data";
export function Form({
  schema,
  value,
  onChange,
  errors = {},
  path = "",
  disabled = false,
}: {
  schema: Schema;
  value: Data;
  onChange: (value: Data) => void;
  errors?: Record<string, string>;
  path?: string;
  disabled?: boolean;
}) {
  const type = typeOf(schema);
  if (type === "object" && !Object.keys(schema.properties || {}).length)
    return (
      <Dictionary
        value={value || {}}
        onChange={onChange}
        path={path}
        errors={errors}
        disabled={disabled}
      />
    );
  if (type === "object")
    return (
      <div className="object-fields">
        {Object.entries(schema.properties || {}).map(([key, field]) => (
          <Field
            key={key}
            schema={field}
            value={value?.[key]}
            onChange={(v) => {
              const next = { ...(value || {}) };
              if (v === undefined) delete next[key];
              else next[key] = v;
              onChange(next);
            }}
            required={schema.required?.includes(key)}
            name={key}
            path={path + "/" + key}
            errors={errors}
            disabled={disabled}
          />
        ))}
      </div>
    );
  return (
    <Field
      schema={schema}
      value={value}
      onChange={onChange}
      name={schema.title || "值"}
      path={path}
      errors={errors}
      disabled={disabled}
    />
  );
}
function Field({
  schema,
  value,
  onChange,
  required = false,
  name,
  path,
  errors,
  disabled,
}: {
  schema: Schema;
  value: Data;
  onChange: (v: Data) => void;
  required?: boolean;
  name: string;
  path: string;
  errors: Record<string, string>;
  disabled: boolean;
}) {
  const type = typeOf(schema);
  const label = schema.title || name;
  const error = errors[path];
  const id = "field-" + encodeURIComponent(path);
  const nullable = Array.isArray(schema.type) && schema.type.includes("null");
  return (
    <div
      className={"field " + (error ? "field-error" : "")}
      data-field-path={path}
    >
      <div className="field-heading">
        <label htmlFor={id}>
          {label}
          {required && <span className="required">*</span>}
          {schema["x-unit"] && <span className="unit">{schema["x-unit"]}</span>}
        </label>
        {!required && value !== undefined && (
          <button
            className="text-button"
            type="button"
            onClick={() => onChange(undefined)}
            disabled={disabled}
          >
            清空
          </button>
        )}
      </div>
      {schema.description && (
        <div className="field-description">{schema.description}</div>
      )}
      {schema["x-untyped"] ? (
        <div className="undefined-type">
          请在「编辑模板 → 参数定义」中选择类型
        </div>
      ) : nullable && value === null ? (
        <div className="null-control">
          空值{" "}
          <button
            className="text-button"
            onClick={() => onChange(defaultValue({ ...schema, type }))}
          >
            填写值
          </button>
        </div>
      ) : schema.enum ? (
        <select
          id={id}
          value={value === undefined ? "" : encode(value)}
          onChange={(e) => {
            const item = schema.enum?.find((v) => encode(v) === e.target.value);
            onChange(item);
          }}
          disabled={disabled}
        >
          <option value="">请选择</option>
          {schema.enum.map((v, i) => (
            <option key={i} value={encode(v)}>
              {String(v)}
            </option>
          ))}
        </select>
      ) : type === "object" ? (
        <details className="nested-group" open>
          <summary>
            <ChevronDown size={14} />
            {label}
          </summary>
          <Form
            schema={schema}
            value={value || {}}
            onChange={onChange}
            path={path}
            errors={errors}
            disabled={disabled}
          />
        </details>
      ) : type === "array" ? (
        <div className="array-field">
          {(Array.isArray(value) ? value : []).map((item, index) => (
            <div className="array-row" key={index}>
              <div className="array-row-head">
                <span>第 {index + 1} 项</span>
                <button
                  className="icon-button"
                  aria-label={`删除${label}第${index + 1}项`}
                  onClick={() =>
                    onChange(value.filter((_: Data, i: number) => i !== index))
                  }
                  disabled={disabled}
                >
                  <Trash2 size={14} />
                </button>
              </div>
              <Form
                schema={schema.items || { type: "string" }}
                value={item}
                onChange={(v) =>
                  onChange(
                    value.map((x: Data, i: number) => (i === index ? v : x)),
                  )
                }
                path={path + "/" + index}
                errors={errors}
                disabled={disabled}
              />
            </div>
          ))}
          {value === undefined && (
            <button className="text-button" onClick={() => onChange([])}>
              设为空列表
            </button>
          )}
          <button
            className="add-item"
            type="button"
            onClick={() =>
              onChange([
                ...(value || []),
                defaultValue(schema.items || { type: "string" }) ?? "",
              ])
            }
            disabled={disabled}
          >
            <Plus size={14} />
            添加{label}
          </button>
        </div>
      ) : type === "boolean" && value === undefined ? (
        <select
          id={id}
          value=""
          onChange={(e) => onChange(e.target.value === "true")}
          disabled={disabled}
        >
          <option value="" disabled>
            选择开启或关闭
          </option>
          <option value="true">开启</option>
          <option value="false">关闭</option>
        </select>
      ) : type === "boolean" ? (
        <label className="switch-label">
          <input
            id={id}
            type="checkbox"
            aria-label={label}
            checked={value === true}
            onChange={(e) => onChange(e.target.checked)}
            disabled={disabled}
          />
          <span>{value === true ? "已开启" : "未开启"}</span>
        </label>
      ) : type === "number" || type === "integer" ? (
        <input
          id={id}
          inputMode={type === "integer" ? "numeric" : "decimal"}
          type="text"
          value={numberText(value)}
          placeholder={type === "integer" ? "输入整数" : "输入数值"}
          onChange={(e) =>
            onChange(
              e.target.value === ""
                ? undefined
                : new DraftNumber(e.target.value),
            )
          }
          disabled={disabled}
          aria-invalid={!!error}
        />
      ) : schema.format === "multiline" || name === "source" ? (
        <textarea
          id={id}
          rows={5}
          value={value ?? ""}
          onChange={(e) => onChange(e.target.value)}
          disabled={disabled}
        />
      ) : (
        <input
          id={id}
          type="text"
          value={value ?? ""}
          onChange={(e) => onChange(e.target.value)}
          placeholder={schema["x-placeholder"] || ""}
          disabled={disabled}
          aria-invalid={!!error}
        />
      )}
      {nullable && value !== null && (
        <button
          className="text-button null-button"
          onClick={() => onChange(null)}
        >
          设为空值
        </button>
      )}
      {error && (
        <div role="alert" className="field-message">
          {error}
        </div>
      )}
      {(schema.minimum !== undefined || schema.maximum !== undefined) && (
        <div className="field-description">
          {schema.minimum !== undefined
            ? "最小值 " + numberText(schema.minimum)
            : ""}
          {schema.minimum !== undefined && schema.maximum !== undefined
            ? " · "
            : ""}
          {schema.maximum !== undefined
            ? "最大值 " + numberText(schema.maximum)
            : ""}
        </div>
      )}
    </div>
  );
}

function infer(value: Data): Schema {
  if (
    isLosslessNumber(value) ||
    value instanceof DraftNumber ||
    typeof value === "number"
  )
    return { type: "number" };
  if (typeof value === "boolean") return { type: "boolean" };
  if (Array.isArray(value))
    return {
      type: "array",
      items: value.length ? infer(value[0]) : { type: "string" },
    };
  if (value === null) return { type: ["string", "null"] };
  if (typeof value === "object") return { type: "object" };
  return { type: "string" };
}
function Dictionary({
  value,
  onChange,
  path,
  errors,
  disabled,
}: {
  value: Data;
  onChange: (v: Data) => void;
  path: string;
  errors: Record<string, string>;
  disabled: boolean;
}) {
  const [name, setName] = useState(""),
    [type, setType] = useState("string"),
    [types, setTypes] = useState<Record<string, string>>({}),
    [error, setError] = useState("");
  return (
    <div className="dictionary">
      {Object.entries(value).map(([key, v]) => (
        <Field
          key={key}
          schema={types[key] ? { type: types[key] } : infer(v)}
          name={key}
          value={v}
          path={path + "/" + key}
          errors={errors}
          disabled={disabled}
          onChange={(next) => {
            const object = { ...value };
            if (next === undefined) delete object[key];
            else object[key] = next;
            onChange(object);
          }}
        />
      ))}
      <div className="dictionary-add">
        <input
          aria-label="字段名"
          placeholder="新增字段名"
          value={name}
          onChange={(e) => setName(e.target.value)}
          disabled={disabled}
        />
        <select
          aria-label="字段类型"
          value={type}
          onChange={(e) => setType(e.target.value)}
          disabled={disabled}
        >
          <option value="string">文本</option>
          <option value="number">数值</option>
          <option value="boolean">开关</option>
          <option value="object">对象</option>
          <option value="array">列表</option>
        </select>
        <button
          className="icon-button"
          aria-label="添加字段"
          disabled={disabled}
          onClick={() => {
            const key = name.trim();
            if (!key || Object.prototype.hasOwnProperty.call(value, key)) {
              setError("字段名为空或已存在");
              return;
            }
            setTypes({ ...types, [key]: type });
            onChange({
              ...value,
              [key]:
                type === "number"
                  ? new DraftNumber("0")
                  : type === "boolean"
                    ? false
                    : type === "object"
                      ? {}
                      : type === "array"
                        ? []
                        : "",
            });
            setName("");
            setError("");
          }}
        >
          <Plus size={16} />
        </button>
      </div>
      {error && <div className="field-message">{error}</div>}
    </div>
  );
}
