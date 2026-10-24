import { useState } from "react";
import {
  Button,
  Card,
  Collapse,
  Form,
  Input,
  Select,
  Space,
  Switch,
  Typography,
} from "antd";
import { PlusOutlined, DeleteOutlined, UndoOutlined } from "@ant-design/icons";
import {
  decode,
  encode,
  numeric,
  numericText,
  object,
  pointer,
  type Data,
  type Schema,
} from "../lib/json";

interface Props {
  schema: Schema;
  value: Data;
  onChange: (value: Data) => void;
  onField?: (path: (string | number)[], value: Data) => void;
  path?: (string | number)[];
  invalid?: Record<string, string>;
  onInvalid?: (path: (string | number)[], raw: string | undefined) => void;
  origins?: Record<string, string>;
  disabled?: boolean;
}
export function SchemaFields(props: Props) {
  const {
    schema,
    value,
    path = [],
    onChange,
    onField,
    invalid = {},
    onInvalid,
    origins = {},
    disabled,
  } = props;
  const [newKey, setNewKey] = useState("");
  const spec = typeof schema === "object" ? schema : {};
  const type =
    (Array.isArray(spec.type)
      ? spec.type.find((item: string) => item !== "null")
      : spec.type) || (object(value) ? "object" : undefined);
  const nullable = Array.isArray(spec.type) && spec.type.includes("null");
  const location = pointer(path);
  const change = (next: Data) => {
    onInvalid?.(path, undefined);
    if (onField) onField(path, next);
    else onChange(next);
  };
  if (schema === false)
    return <Typography.Text type="danger">此字段不允许输入</Typography.Text>;
  if (value === null && nullable)
    return (
      <Space wrap size={[12, 8]}>
        <Typography.Text code>null</Typography.Text>
        <Button
          disabled={disabled}
          onClick={() =>
            change(
              type === "object"
                ? {}
                : type === "array"
                  ? []
                  : type === "boolean"
                    ? false
                    : "",
            )
          }
        >
          填写值
        </Button>
      </Space>
    );
  if (spec.enum)
    return (
      <Select
        aria-label={spec.title || location || "取值"}
        disabled={disabled}
        allowClear
        value={value === undefined ? undefined : encode(value)}
        options={spec.enum.map((entry: Data) => ({
          value: encode(entry),
          label: typeof entry === "string" ? entry : encode(entry),
        }))}
        onChange={(raw) => change(raw === undefined ? undefined : decode(raw))}
      />
    );
  if (type === "object") {
    const properties: Record<string, Schema> = spec.properties || {};
    const keys = [
      ...new Set([
        ...Object.keys(properties),
        ...Object.keys(object(value) ? value : {}),
      ]),
    ];
    return (
      <div className="schema-fields">
        {keys.map((key) => {
          const field: Data =
            properties[key] ??
            (typeof spec.additionalProperties === "object"
              ? spec.additionalProperties
              : {});
          const child = [...path, key],
            fieldPath = pointer(child);
          const title = typeof field === "object" ? field.title || key : key;
          const origin = origins[fieldPath] || origins[key];
          return (
            <Form.Item
              key={key}
              label={
                <Space wrap size={[12, 8]}>
                  {title}
                  {origin && (
                    <Typography.Text type="secondary">
                      {origin === "manual" ? "本次填写" : origin}
                    </Typography.Text>
                  )}
                </Space>
              }
              required={spec.required?.includes(key)}
              htmlFor={"field-" + fieldPath}
              extra={field.description}
              validateStatus={
                Object.hasOwn(invalid, fieldPath) ? "error" : undefined
              }
              help={
                Object.hasOwn(invalid, fieldPath) ? "请修正当前输入" : undefined
              }
            >
              <div data-field-path={fieldPath}>
                <SchemaFields
                  {...props}
                  schema={field}
                  value={value?.[key]}
                  path={child}
                  onChange={(next) => {
                    const copy = { ...(object(value) ? value : {}) };
                    if (next === undefined) delete copy[key];
                    else
                      Object.defineProperty(copy, key, {
                        value: next,
                        enumerable: true,
                        configurable: true,
                        writable: true,
                      });
                    onChange(copy);
                  }}
                />
                <Space size="small">
                  {onField && (
                    <Button
                      size="small"
                      type="link"
                      icon={<UndoOutlined aria-hidden />}
                      disabled={disabled}
                      onClick={() => {
                        onInvalid?.(child, undefined);
                        onField(child, undefined);
                      }}
                    >
                      恢复跟随
                    </Button>
                  )}
                  {!Object.hasOwn(properties, key) && (
                    <Button
                      size="small"
                      type="text"
                      danger
                      disabled={disabled}
                      onClick={() => {
                        if (onField) onField(child, undefined);
                        else {
                          const copy = { ...value };
                          delete copy[key];
                          onChange(copy);
                        }
                      }}
                    >
                      删除字段
                    </Button>
                  )}
                </Space>
              </div>
            </Form.Item>
          );
        })}
        {spec.additionalProperties !== false && (
          <Space.Compact block>
            <Input
              aria-label={"新增字段名" + location}
              placeholder="新增字段名"
              value={newKey}
              disabled={disabled}
              onChange={(event) => setNewKey(event.target.value)}
            />
            <Button
              icon={<PlusOutlined aria-hidden />}
              disabled={
                disabled || !newKey || Object.hasOwn(value || {}, newKey)
              }
              onClick={() => {
                if (onField) onField([...path, newKey], "");
                else
                  onChange(
                    Object.fromEntries([
                      ...Object.entries(value || {}),
                      [newKey, ""],
                    ]),
                  );
                setNewKey("");
              }}
            >
              添加
            </Button>
          </Space.Compact>
        )}
      </div>
    );
  }
  if (type === "array")
    return (
      <Form
        component={false}
        fields={[{ name: ["items"], value: Array.isArray(value) ? value : [] }]}
        onValuesChange={(_, values) => change(values.items)}
      >
        <Form.List name="items">
          {(fields, { add, remove }) => (
            <Space vertical style={{ width: "100%" }}>
              {fields.map((field) => (
                <Card
                  size="small"
                  key={field.key}
                  title={`第 ${field.name + 1} 项`}
                  extra={
                    <Button
                      aria-label="删除数组项"
                      icon={<DeleteOutlined aria-hidden />}
                      disabled={disabled}
                      onClick={() => remove(field.name)}
                    />
                  }
                >
                  <Form.Item name={[field.name]} noStyle>
                    <ArrayItem
                      schema={spec.items || {}}
                      path={[...path, field.name]}
                      invalid={invalid}
                      onInvalid={onInvalid}
                      disabled={disabled}
                    />
                  </Form.Item>
                </Card>
              ))}
              <Button
                icon={<PlusOutlined aria-hidden />}
                disabled={disabled}
                onClick={() =>
                  add(
                    spec.items?.type === "object"
                      ? {}
                      : spec.items?.type === "array"
                        ? []
                        : spec.items?.type === "boolean"
                          ? false
                          : "",
                  )
                }
              >
                添加一项
              </Button>
            </Space>
          )}
        </Form.List>
      </Form>
    );
  if (type === "boolean")
    return (
      <Switch
        aria-label={spec.title || location || "布尔值"}
        checked={value === true}
        disabled={disabled}
        onChange={change}
      />
    );
  if (type === "null") return <Typography.Text code>null</Typography.Text>;
  if (type === "number" || type === "integer")
    return (
      <Input
        id={"field-" + location}
        aria-label={spec.title || location || "数值"}
        inputMode="decimal"
        disabled={disabled}
        value={
          Object.hasOwn(invalid, location)
            ? invalid[location]
            : numericText(value)
        }
        onChange={(event) => {
          const raw = event.target.value;
          try {
            const next = numeric(raw);
            change(next);
          } catch {
            onInvalid?.(path, raw);
          }
        }}
      />
    );
  if (type === "string")
    return (
      <Space.Compact block>
        <Input
          id={"field-" + location}
          aria-label={spec.title || location || "文本"}
          disabled={disabled}
          value={value ?? ""}
          onChange={(event) => change(event.target.value)}
        />
        {nullable && (
          <Button disabled={disabled} onClick={() => change(null)}>
            null
          </Button>
        )}
      </Space.Compact>
    );
  return (
    <Input.TextArea
      aria-label={spec.title || location || "JSON 值"}
      disabled={disabled}
      value={
        Object.hasOwn(invalid, location)
          ? invalid[location]
          : value === undefined
            ? ""
            : encode(value, true)
      }
      autoSize={{ minRows: 2, maxRows: 8 }}
      onChange={(event) => {
        try {
          change(decode(event.target.value));
        } catch {
          onInvalid?.(path, event.target.value);
        }
      }}
    />
  );
}
function ArrayItem(
  props: Omit<Props, "value" | "onChange"> & {
    value?: Data;
    onChange?: (value: Data) => void;
  },
) {
  return (
    <SchemaFields
      {...props}
      value={props.value}
      onChange={props.onChange || (() => {})}
    />
  );
}
export function SchemaDesigner({
  raw,
  onChange,
}: {
  raw: string;
  onChange: (raw: string) => void;
}) {
  const [name, setName] = useState("");
  let schema: Data;
  try {
    schema = decode(raw);
  } catch {
    return (
      <Typography.Text type="danger">
        请先在高级 JSON 中修正参数定义
      </Typography.Text>
    );
  }
  if (!object(schema))
    return (
      <Typography.Text>
        当前使用布尔 Schema，可在高级 JSON 中编辑。
      </Typography.Text>
    );
  const properties = schema.properties || {};
  const update = (key: string, patch: Data) =>
    onChange(
      encode(
        {
          ...schema,
          properties: {
            ...properties,
            [key]: { ...properties[key], ...patch },
          },
        },
        true,
      ),
    );
  return (
    <Space vertical style={{ width: "100%" }}>
      <Collapse
        items={Object.entries(properties).map(([key, value]) => {
          const field = value as Data;
          return {
            key,
            label: field.title || key,
            children: (
              <Form layout="vertical">
                <Form.Item label="字段标识">
                  <Input value={key} disabled />
                </Form.Item>
                <Form.Item label="显示名称">
                  <Input
                    value={field.title || ""}
                    onChange={(event) =>
                      update(key, { title: event.target.value })
                    }
                  />
                </Form.Item>
                <Form.Item label="参数类型">
                  <Select
                    aria-label={key + " 参数类型"}
                    value={field.type}
                    placeholder="请选择类型"
                    options={[
                      "string",
                      "number",
                      "integer",
                      "boolean",
                      "object",
                      "array",
                      "null",
                    ].map((type) => ({ value: type, label: type }))}
                    onChange={(type) => {
                      const next = { ...field, type };
                      delete next["x-untyped"];
                      onChange(
                        encode(
                          {
                            ...schema,
                            properties: { ...properties, [key]: next },
                          },
                          true,
                        ),
                      );
                    }}
                  />
                </Form.Item>
                <Form.Item label="必填">
                  <Switch
                    checked={schema.required?.includes(key)}
                    onChange={(checked) =>
                      onChange(
                        encode(
                          {
                            ...schema,
                            required: [
                              ...new Set([
                                ...(schema.required || []).filter(
                                  (entry: string) => entry !== key,
                                ),
                                ...(checked ? [key] : []),
                              ]),
                            ],
                          },
                          true,
                        ),
                      )
                    }
                  />
                </Form.Item>
                <Form.Item label="单位">
                  <Input
                    value={field["x-unit"] || ""}
                    onChange={(event) =>
                      update(key, { "x-unit": event.target.value })
                    }
                  />
                </Form.Item>
                <Form.Item label="说明">
                  <Input
                    value={field.description || ""}
                    onChange={(event) =>
                      update(key, { description: event.target.value })
                    }
                  />
                </Form.Item>
                <Typography.Paragraph type="secondary">
                  范围、枚举及其他 Schema 约束可在「高级
                  JSON」编辑，未完成内容会保留并阻止保存。
                </Typography.Paragraph>
                {field.type === "object" && (
                  <SchemaDesigner
                    raw={encode(field, true)}
                    onChange={(next) =>
                      onChange(
                        encode(
                          {
                            ...schema,
                            properties: { ...properties, [key]: decode(next) },
                          },
                          true,
                        ),
                      )
                    }
                  />
                )}
                {field.type === "array" && (
                  <>
                    <Form.Item label="数组元素类型">
                      <Select
                        aria-label={key + " 数组元素类型"}
                        value={field.items?.type}
                        placeholder="请选择元素类型"
                        options={[
                          "string",
                          "number",
                          "integer",
                          "boolean",
                          "object",
                          "array",
                          "null",
                        ].map((type) => ({ value: type, label: type }))}
                        onChange={(type) =>
                          update(key, { items: { ...field.items, type } })
                        }
                      />
                    </Form.Item>
                    {field.items?.type === "object" && (
                      <SchemaDesigner
                        raw={encode(field.items, true)}
                        onChange={(next) =>
                          update(key, { items: decode(next) })
                        }
                      />
                    )}
                  </>
                )}
                <Button
                  danger
                  onClick={() => {
                    const next = { ...properties };
                    delete next[key];
                    onChange(
                      encode(
                        {
                          ...schema,
                          properties: next,
                          required: (schema.required || []).filter(
                            (item: string) => item !== key,
                          ),
                        },
                        true,
                      ),
                    );
                  }}
                >
                  删除参数
                </Button>
              </Form>
            ),
          };
        })}
      />
      <Space.Compact block>
        <Input
          placeholder="新增参数标识"
          value={name}
          onChange={(event) => setName(event.target.value)}
        />
        <Button
          disabled={!name || Object.hasOwn(properties, name)}
          onClick={() => {
            update(name, { title: name, "x-untyped": true });
            setName("");
          }}
        >
          添加参数
        </Button>
      </Space.Compact>
    </Space>
  );
}
