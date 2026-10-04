import { useState } from "react";
import { Plus, Trash2, ChevronDown } from "lucide-react";
import {
  DraftNumber,
  numberText,
  typeOf,
  type Schema,
  type Data,
} from "../lib/data";
const TYPES = [
  ["string", "文本"],
  ["number", "数值"],
  ["integer", "整数"],
  ["boolean", "开关"],
  ["object", "对象 / 分组"],
  ["array", "数组 / 列表"],
];
export function SchemaEditor({
  schema,
  onChange,
  onInsert,
}: {
  schema: Schema;
  onChange: (s: Schema) => void;
  onInsert?: (name: string) => void;
}) {
  const [newName, setNewName] = useState("");
  const [error, setError] = useState("");
  function add() {
    const name = newName.trim();
    if (!name) {
      setError("请输入参数名");
      return;
    }
    if (schema.properties?.[name]) {
      setError("参数名已存在");
      return;
    }
    onChange({
      ...schema,
      type: "object",
      properties: {
        ...(schema.properties || {}),
        [name]: { title: name, "x-untyped": true },
      },
    });
    setNewName("");
    setError("");
  }
  return (
    <div className="schema-editor">
      {Object.entries(schema.properties || {}).map(([name, field]) => (
        <Definition
          key={name}
          name={name}
          field={field}
          required={schema.required?.includes(name) || false}
          change={(next) =>
            onChange({
              ...schema,
              properties: { ...schema.properties, [name]: next },
            })
          }
          require={(yes) =>
            onChange({
              ...schema,
              required: yes
                ? [...new Set([...(schema.required || []), name])]
                : (schema.required || []).filter((x) => x !== name),
            })
          }
          remove={() => {
            const properties = { ...schema.properties };
            delete properties[name];
            onChange({
              ...schema,
              properties,
              required: (schema.required || []).filter((x) => x !== name),
            });
          }}
          onInsert={onInsert}
        />
      ))}
      <div className="new-field">
        <input
          aria-label="新参数名"
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          placeholder="参数名，例如 feed、user"
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add();
            }
          }}
        />
        <button onClick={add} className="button secondary">
          <Plus size={14} />
          添加参数
        </button>
      </div>
      {error && <div className="field-message">{error}</div>}
      {!Object.keys(schema.properties || {}).length && (
        <p className="hint">
          从源码识别变量，或添加一个参数。类型由你明确选择。
        </p>
      )}
    </div>
  );
}
function Definition({
  name,
  field,
  required,
  change,
  require,
  remove,
  onInsert,
}: {
  name: string;
  field: Schema;
  required: boolean;
  change: (s: Schema) => void;
  require: (b: boolean) => void;
  remove: () => void;
  onInsert?: (name: string) => void;
}) {
  const type = field["x-untyped"] ? "unknown" : typeOf(field);
  function patch(key: string, value: Data) {
    const next = { ...field };
    if (value === undefined || value === "") delete next[key];
    else next[key] = value;
    change(next);
  }
  function setType(type: string) {
    const next: Schema = { ...field, type };
    delete next["x-untyped"];
    if (type === "object") next.properties = next.properties || {};
    if (type === "array") next.items = next.items || { type: "string" };
    if (type !== "object") delete next.properties;
    if (type !== "array") delete next.items;
    change(next);
  }
  return (
    <details className="definition" open={type === "unknown"}>
      <summary>
        <ChevronDown size={14} />
        <span className="definition-name">{field.title || name}</span>
        <code>{name}</code>
        <span className={"type-tag " + (type === "unknown" ? "warning" : "")}>
          {TYPES.find(([t]) => t === type)?.[1] || "待定义"}
        </span>
      </summary>
      <div className="definition-body">
        <div className="two-fields">
          <label>
            显示名称
            <input
              value={field.title || ""}
              onChange={(e) => patch("title", e.target.value)}
              placeholder={name}
            />
          </label>
          <label>
            类型
            <select
              aria-label={name + "类型"}
              value={type}
              onChange={(e) => setType(e.target.value)}
            >
              <option value="unknown" disabled>
                请选择类型
              </option>
              {TYPES.map(([key, label]) => (
                <option key={key} value={key}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        </div>
        <label>
          说明
          <input
            value={field.description || ""}
            onChange={(e) => patch("description", e.target.value)}
            placeholder="说明这个参数的用途"
          />
        </label>
        <div className="definition-options">
          <label className="checkbox">
            <input
              type="checkbox"
              checked={required}
              onChange={(e) => require(e.target.checked)}
            />
            必填参数
          </label>
          {onInsert && (
            <button className="text-button" onClick={() => onInsert(name)}>
              插入变量
            </button>
          )}
          <button className="text-button danger" onClick={remove}>
            <Trash2 size={12} />
            删除
          </button>
        </div>
        {(type === "number" || type === "integer") && (
          <div className="three-fields">
            <label>
              单位
              <input
                value={field["x-unit"] || ""}
                onChange={(e) => patch("x-unit", e.target.value)}
                placeholder="mm"
              />
            </label>
            <label>
              最小值
              <input
                value={numberText(field.minimum)}
                onChange={(e) =>
                  patch(
                    "minimum",
                    e.target.value
                      ? new DraftNumber(e.target.value)
                      : undefined,
                  )
                }
                inputMode="decimal"
              />
            </label>
            <label>
              最大值
              <input
                value={numberText(field.maximum)}
                onChange={(e) =>
                  patch(
                    "maximum",
                    e.target.value
                      ? new DraftNumber(e.target.value)
                      : undefined,
                  )
                }
                inputMode="decimal"
              />
            </label>
          </div>
        )}
        {type === "object" && (
          <SchemaEditor
            schema={field}
            onChange={change}
            onInsert={onInsert ? (n) => onInsert(name + "." + n) : undefined}
          />
        )}
        {type === "array" && (
          <>
            <label>
              列表元素类型
              <select
                aria-label={name + "元素类型"}
                value={typeOf(field.items || {})}
                onChange={(e) =>
                  patch("items", {
                    type: e.target.value,
                    ...(e.target.value === "object"
                      ? { properties: {}, required: [] }
                      : {}),
                  })
                }
              >
                {TYPES.map(([key, label]) => (
                  <option key={key} value={key}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            {typeOf(field.items || {}) === "object" && (
              <SchemaEditor
                schema={field.items!}
                onChange={(items) => patch("items", items)}
              />
            )}
          </>
        )}
        {type === "string" && (
          <label>
            候选值（每行一个，留空则自由填写）
            <textarea
              rows={3}
              value={(field.enum || []).join("\n")}
              onChange={(e) =>
                patch(
                  "enum",
                  e.target.value ? e.target.value.split("\n") : undefined,
                )
              }
            />
          </label>
        )}
      </div>
    </details>
  );
}
