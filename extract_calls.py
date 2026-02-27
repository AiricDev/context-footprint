import json
with open("/Users/leric/code/backup/LLMRelay/semantic.json", "r") as f:
    data = json.load(f)

for doc in data["documents"]:
    if "images.py" in doc["relative_path"]:
        for ref in doc["references"]:
            if ref.get("enclosing_symbol") == "app.api.openai.images.create_image_edit" and ref.get("role") == "Call":
                print(f"Call: target={ref.get('target_symbol')}, method={ref.get('method_name')}")
