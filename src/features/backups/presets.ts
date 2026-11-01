/** Usual "show the configuration" commands, with the pager already disabled
 * where the device allows it on the command line. */
export const backupPresets: { label: string; command: string }[] = [
  { label: "Cisco IOS", command: "show running-config" },
  { label: "Huawei VRP", command: "display current-configuration" },
  { label: "Juniper", command: "show configuration | display set | no-more" },
  { label: "MikroTik", command: "/export" },
  { label: "Linux", command: "cat /etc/network/interfaces" },
];

export const DEFAULT_BACKUP_COMMAND = backupPresets[0].command;
