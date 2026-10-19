"use client";

// 通知時間帯の時刻入力（HH:MM）。設定画面と初回起動の案内で同じ操作感にするため共有する。
// 値の妥当性は保存時に Rust 側（UserSettingsDto::validate）で検証する。
export function TimeInput({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className="flex items-center gap-1 bg-white border border-border rounded-lg px-3 py-1.5">
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="w-12 text-sm text-center bg-transparent outline-none"
      />
      <div className="flex flex-col">
        <button
          className="text-muted-foreground hover:text-foreground text-[10px] leading-none"
          onClick={() => {
            const [h, m] = value.split(":").map(Number);
            const newH = (h + 1) % 24;
            onChange(`${String(newH).padStart(2, "0")}:${String(m).padStart(2, "0")}`);
          }}
        >
          ▲
        </button>
        <button
          className="text-muted-foreground hover:text-foreground text-[10px] leading-none"
          onClick={() => {
            const [h, m] = value.split(":").map(Number);
            const newH = (h - 1 + 24) % 24;
            onChange(`${String(newH).padStart(2, "0")}:${String(m).padStart(2, "0")}`);
          }}
        >
          ▼
        </button>
      </div>
    </div>
  );
}
