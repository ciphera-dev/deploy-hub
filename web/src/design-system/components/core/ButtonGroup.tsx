import { Children, cloneElement, Fragment, isValidElement, type CSSProperties, type ReactElement, type ReactNode } from "react";

export interface ButtonGroupProps {
  /** Select bare / Button variant="ghost" en enfants, separes par des filets */
  children: ReactNode;
  style?: CSSProperties;
}

type StyledProps = { style?: CSSProperties };

export function ButtonGroup({ children, style }: ButtonGroupProps) {
  const items = Children.toArray(children);
  return (
    <div style={{ display: "inline-flex", alignItems: "center", height: 32, borderRadius: "var(--radius-md)", background: "var(--surface-control)", border: "1px solid var(--border-strong)", boxShadow: "var(--shadow-control)", ...style }}>
      {items.map((c, i) => (
        <Fragment key={i}>
          {i > 0 && <span style={{ width: 1, alignSelf: "stretch", background: "var(--border-strong)" }} />}
          {isValidElement<StyledProps>(c) ? cloneElement(c as ReactElement<StyledProps>, { style: { ...(c.props.style ?? {}), height: 30 } }) : c}
        </Fragment>
      ))}
    </div>
  );
}
