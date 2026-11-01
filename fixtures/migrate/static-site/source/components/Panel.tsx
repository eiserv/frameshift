import styles from './Panel.module.css';

export function Panel({ children }: { children: React.ReactNode }) {
  return <section className={styles.panel}>{children}</section>;
}
