"""Quick test of different rembg models on the same sprite. Run via bazel run."""
from rembg import remove, new_session

inputs = ["/tmp/sprite_raw.png", "/tmp/sprite_whitebg.png"]

for model in ["u2net", "isnet-anime", "isnet-general-use"]:
    print(f"Testing model: {model}")
    session = new_session(model)
    for path in inputs:
        variant = path.split("_")[-1].replace(".png", "")
        out = f"/tmp/sprite_{model.replace('-','_')}_{variant}.png"
        with open(path, "rb") as f:
            data = f.read()
        result = remove(data, session=session)
        with open(out, "wb") as f:
            f.write(result)
        print(f"  saved {out}")
