/** 标题 → 网址里用的短名：小写，空格换成 `-`。 */
export function slug(title: string): string {
  return title.toLowerCase().replace(" ", "-");
}
