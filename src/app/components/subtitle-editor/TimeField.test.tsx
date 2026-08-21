import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { TimeField } from "./TimeField";

const START_MS = 1000;
const START_LABEL = "00:00:01,000";
const NEXT_MS = 2000;
const NEXT_LABEL = "00:00:02,000";

function renderField(
  props: Partial<ComponentProps<typeof TimeField>> = {},
) {
  const onCommit = vi.fn();
  const onReject = vi.fn();
  const onCancel = vi.fn();
  const onFocusCue = vi.fn();
  const defaults: ComponentProps<typeof TimeField> = {
    valueMs: START_MS,
    label: "Start time",
    canEdit: true,
    invalid: false,
    onCommit,
    onReject,
    onCancel,
    onFocusCue,
  };
  const view = render(<TimeField {...defaults} {...props} />);
  return {
    ...view,
    onCommit,
    onReject,
    onCancel,
    onFocusCue,
    input: screen.getByLabelText("Start time") as HTMLInputElement,
    rerenderField: (next: Partial<ComponentProps<typeof TimeField>> = {}) =>
      view.rerender(<TimeField {...defaults} {...props} {...next} />),
  };
}

describe("TimeField", () => {
  it("shows the formatted committed time while idle", () => {
    const { input } = renderField();
    expect(input.value).toBe(START_LABEL);
  });

  it("follows valueMs while idle", () => {
    const { input, rerenderField } = renderField();
    rerenderField({ valueMs: NEXT_MS });
    expect(input.value).toBe(NEXT_LABEL);
  });

  it("does not commit on blur when the value was not edited", () => {
    const { input, onCommit, onReject } = renderField();
    fireEvent.focus(input);
    fireEvent.blur(input);
    expect(onCommit).not.toHaveBeenCalled();
    expect(onReject).not.toHaveBeenCalled();
  });

  it("commits milliseconds on blur when the draft parses", () => {
    const { input, onCommit, onReject, rerenderField } = renderField();
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: NEXT_LABEL } });
    fireEvent.blur(input);
    expect(onCommit).toHaveBeenCalledWith(NEXT_MS);
    expect(onReject).not.toHaveBeenCalled();
    rerenderField({ valueMs: NEXT_MS });
    expect(input.value).toBe(NEXT_LABEL);
  });

  it("keeps the typed draft and rejects when blur cannot parse", () => {
    const { input, onCommit, onReject, rerenderField } = renderField();
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "nope" } });
    fireEvent.blur(input);
    expect(onReject).toHaveBeenCalledTimes(1);
    expect(onCommit).not.toHaveBeenCalled();
    expect(input.value).toBe("nope");
    rerenderField({ valueMs: NEXT_MS, invalid: true });
    expect(input.value).toBe("nope");
  });

  it("retries the kept draft on a later blur", () => {
    const { input, onCommit, onReject } = renderField();
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "nope" } });
    fireEvent.blur(input);
    expect(onReject).toHaveBeenCalledTimes(1);

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: NEXT_LABEL } });
    fireEvent.blur(input);
    expect(onCommit).toHaveBeenCalledWith(NEXT_MS);
    expect(onReject).toHaveBeenCalledTimes(1);
  });

  it("cancels the draft on Escape without committing", () => {
    const { input, onCommit, onReject, onCancel } = renderField();
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "nope" } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onCommit).not.toHaveBeenCalled();
    expect(onReject).not.toHaveBeenCalled();
    expect(input.value).toBe(START_LABEL);
  });

  it("commits and blurs on Enter when the draft parses", () => {
    const { input, onCommit, onReject } = renderField();
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: NEXT_LABEL } });
    const blur = vi.spyOn(input, "blur");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith(NEXT_MS);
    expect(onReject).not.toHaveBeenCalled();
    expect(blur).toHaveBeenCalled();
  });

  it("does not blur on Enter when the draft is invalid", () => {
    const { input, onCommit, onReject } = renderField();
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "nope" } });
    const blur = vi.spyOn(input, "blur");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onReject).toHaveBeenCalledTimes(1);
    expect(onCommit).not.toHaveBeenCalled();
    expect(input.value).toBe("nope");
    expect(blur).not.toHaveBeenCalled();
  });
});
