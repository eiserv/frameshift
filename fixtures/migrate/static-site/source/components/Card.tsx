export const Card = ({ title, tone = 'plain' }: { title: string; tone?: string }) => {
  const label = `${title} (${tone})`;
  return <article className={tone}>{`${label}`}</article>;
};
