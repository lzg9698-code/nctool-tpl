// @vitest-environment jsdom
import { useState } from "react";
import { beforeAll, afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Form } from "antd";
import { SchemaFields } from "./SchemaFields";
import { decode, encode, pointer, type Data } from "../lib/json";
beforeAll(() => {
  Object.defineProperty(window, "matchMedia", {
    value: vi.fn().mockImplementation((query) => ({
      matches: false,
      media: query,
      addListener() {},
      removeListener() {},
      addEventListener() {},
      removeEventListener() {},
      dispatchEvent() {
        return true;
      },
    })),
  });
});
afterEach(cleanup);
function Fixture({ initial = '{"n":9007199254740993}' }: { initial?: string }) {
  const [value, setValue] = useState<Data>(() => decode(initial)),
    [invalid, setInvalid] = useState<Record<string, string>>({});
  return (
    <Form component="div" layout="vertical">
      <SchemaFields
        schema={{
          type: "object",
          properties: {
            n: { type: "integer", title: "大整数" },
            nullable: { type: ["string", "null"], title: "可空值" },
          },
          additionalProperties: false,
        }}
        value={value}
        onChange={setValue}
        invalid={invalid}
        onInvalid={(path, raw) =>
          setInvalid((current) => {
            const next = { ...current };
            if (raw === undefined) delete next[pointer(path)];
            else next[pointer(path)] = raw;
            return next;
          })
        }
      />
      <output data-testid="payload">{encode(value)}</output>
      <button disabled={!!Object.keys(invalid).length}>提交当前输入</button>
    </Form>
  );
}
describe("Schema 控件的业务输入边界", () => {
  it("不使用原生 number 输入，大整数编辑保留完整字面量", () => {
    render(<Fixture />);
    const input = screen.getByRole("textbox", {
      name: "大整数",
    }) as HTMLInputElement;
    expect(input.type).toBe("text");
    expect(input.value).toBe("9007199254740993");
    fireEvent.change(input, { target: { value: "18446744073709551615" } });
    expect(screen.getByTestId("payload").textContent).toContain(
      "18446744073709551615",
    );
  });
  it("不完整数字保持可见并阻止提交，修正后解除门禁", () => {
    render(<Fixture />);
    const input = screen.getByRole("textbox", {
      name: "大整数",
    }) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "1e" } });
    expect(input.value).toBe("1e");
    expect(
      (
        screen.getByRole("button", {
          name: "提交当前输入",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    fireEvent.change(input, { target: { value: "42" } });
    expect(input.value).toBe("42");
    expect(
      (
        screen.getByRole("button", {
          name: "提交当前输入",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
    expect(screen.getByTestId("payload").textContent).toContain('"n":42');
  });
  it("null 与空字符串保持不同语义", () => {
    render(<Fixture initial='{"n":1,"nullable":null}' />);
    expect(screen.getByText("null", { exact: true })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "填写值" }));
    expect(screen.getByTestId("payload").textContent).toContain(
      '"nullable":""',
    );
  });
});
