// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Button, ConfigProvider } from "antd";
import { buttonConfig } from "./buttonConfig";

afterEach(cleanup);
describe("按钮加载状态的可访问名称",()=>{
  it("加载和快速结束时保持动作名称，加载时仍阻止重复执行",()=>{
    let clicks=0;
    const view=(loading:boolean)=><ConfigProvider button={buttonConfig}><Button loading={loading} onClick={()=>clicks++}>执行操作</Button></ConfigProvider>;
    const fixture=render(view(true));
    fireEvent.click(screen.getByRole("button",{name:"执行操作"}));
    expect(clicks).toBe(0);
    expect(screen.queryByRole("img",{name:"loading"})).toBeNull();
    fixture.rerender(view(false));
    fireEvent.click(screen.getByRole("button",{name:"执行操作"}));
    expect(clicks).toBe(1);
    fixture.rerender(view(true));
    expect(screen.getByRole("button",{name:"执行操作"})).toBeTruthy();
    fixture.rerender(view(false));
    expect(screen.getByRole("button",{name:"执行操作"})).toBeTruthy();
  });
});
