class RelayImageUseCase:
    def execute(self):
        pass

def get_relay_image_use_case() -> RelayImageUseCase:
    return RelayImageUseCase()

from fastapi import Depends

def create_image_edit(
    use_case: RelayImageUseCase = Depends(get_relay_image_use_case),
):
    use_case.execute()
