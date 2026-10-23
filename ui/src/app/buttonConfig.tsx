import { LoadingOutlined } from "@ant-design/icons";

// Loading is a visual state, not part of the button's accessible action name.
// Also covers a loading icon retained while its leave transition completes.
export const buttonConfig = { loadingIcon: <LoadingOutlined aria-hidden spin /> };
