import { isTauri } from "@tauri-apps/api/core";
import { Bluetooth, BluetoothConnected, Settings2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { type BtDevice, commands } from "@/shared/ipc/bindings";
import { patchSettings } from "@/shared/ipc/settings";
import { Menu, type MenuItem } from "@/shared/ui/Menu";
import { notify } from "@/shared/ui/notify";

/**
 * The Bluetooth menu in the player (T5.4, FR-WIN-006/007, `07` §6): the paired audio devices
 * with «Подключить» / «Отключить», «Играть через …» for a connected one (only Sirtone's output,
 * the Windows default stays) and «Параметры Bluetooth» as the last resort. Next to Windows'
 * Bluetooth, never instead of it. The list is read when the menu opens.
 */
export function BluetoothButton() {
  const { t } = useTranslation("shell");
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  const [list, setList] = useState<BtDevice[] | null>(null);
  if (!isTauri()) return null;
  const open = (x: number, y: number) => {
    setAt({ x, y });
    setList(null);
    void commands.btDevices().then((r) => setList(r.status === "ok" ? r.data : []));
  };
  const set = (d: BtDevice, connect: boolean) => {
    notify(t(connect ? "bt.connecting" : "bt.disconnecting", { name: d.name }));
    void commands.btSet(d.name, connect).then((r) => {
      if (r.status === "ok" && r.data) return;
      notify(t("bt.failed", { name: d.name }), {
        label: t("bt.settings"),
        onClick: () => void commands.btOpenSettings(),
      });
    });
  };
  const playThrough = (d: BtDevice) => {
    void commands.audioDevices().then((outs) => {
      const out = outs.find((o) => d.endpoints.includes(o.id));
      if (!out) return notify(t("bt.noOutput", { name: d.name }));
      patchSettings({ playback: { output: { device: out.id } } })
        .then(() => notify(t("bt.playing", { name: d.name })))
        .catch(() => {});
    });
  };
  const items: MenuItem[] =
    list === null
      ? [{ id: "loading", label: t("bt.loading"), disabled: true }]
      : [
          ...(list.length === 0
            ? [{ id: "none", label: t("bt.none"), disabled: true } as MenuItem]
            : list.map(
                (d): MenuItem => ({
                  id: d.name,
                  label: d.name,
                  icon: d.connected ? BluetoothConnected : Bluetooth,
                  submenu: d.connected
                    ? [
                        {
                          id: "play",
                          label: t("bt.playThrough"),
                          onSelect: () => playThrough(d),
                        },
                        { id: "off", label: t("bt.disconnect"), onSelect: () => set(d, false) },
                      ]
                    : [{ id: "on", label: t("bt.connect"), onSelect: () => set(d, true) }],
                }),
              )),
          { separator: true },
          {
            id: "settings",
            label: t("bt.settings"),
            icon: Settings2,
            onSelect: () => void commands.btOpenSettings(),
          },
        ];
  return (
    <>
      <button
        type="button"
        aria-label={t("bt.button")}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          open(r.left, r.top);
        }}
        className="flex size-9 items-center justify-center rounded-full text-icon hover:bg-bg-hover"
      >
        <Bluetooth size={20} strokeWidth={1.5} />
      </button>
      {at && <Menu items={items} at={at} label={t("bt.button")} onClose={() => setAt(null)} />}
    </>
  );
}
