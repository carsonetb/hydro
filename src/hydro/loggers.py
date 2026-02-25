import logging 

LOG_LEVEL = logging.DEBUG

FORMATTER = logging.Formatter("[%(asctime)s] [%(name)s] [%(levelname)s] %(message)s")
FORMATTER_INTERNAL = logging.Formatter("[%(asctime)s] [%(name)s] [INTERNAL %(levelname)s] [at %(lineno)d] %(message)s")

def create(name: str):
    logger = logging.getLogger(name)
    handler = logging.StreamHandler()
