import jedi
import os

project_root = "/Users/leric/code/backup/LLMRelay"
file_path = os.path.join(project_root, "app/api/openai/images.py")
with open(file_path, "r") as f:
    source = f.read()

script = jedi.Script(source, path=file_path)

# find line and column of use_case.execute
lines = source.splitlines()
for i, line in enumerate(lines):
    if "use_case.execute(" in line:
        col = line.find("execute")
        lineno = i + 1
        print(f"Found execute at {lineno}:{col}")
        
        print("goto:")
        for d in script.goto(lineno, col):
            print(f"  {d.name} in {d.module_path} (type: {d.type}, full_name: {d.full_name})")
            
        print("infer:")
        for d in script.infer(lineno, col):
            print(f"  {d.name} in {d.module_path} (type: {d.type}, full_name: {d.full_name})")
